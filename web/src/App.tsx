import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, BadgeCheck, Search, Server, Settings2, X } from "lucide-react";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Advanced } from "@/features/Advanced";
import { SocksDashboard } from "@/features/SocksDashboard";
import { SocksManager } from "@/features/SocksManager";
import { CommandPalette } from "@/features/CommandPalette";
import type { SectionId } from "@/features/settingsIndex";
import logo from "@/assets/logo.png";
import { applyLanguage } from "@/core/i18n";
import { useT } from "@/core/useT";
import {
  getCoreLogs, getCoreStatus, isDesktopRuntime, loadProfile, probeCore, runtimeInfo,
  saveProfile as persistProfile, subscribeCore,
} from "@/core/api";
import { withNormalizedEndpoint } from "@/core/endpoint";
import {
  DEFAULT_PROFILE, IDLE_SNAPSHOT, type ConnectionProfile, type CoreLogEvent, type CoreProbe,
  type CoreSnapshot,
} from "@/types";

const MODE_KEY = "nextvpn.mode";
const appVersion = import.meta.env.VITE_APP_VERSION || "unknown";

type Mode = "dashboard" | "socks5" | "advanced";
type Toast = { title: string; message: string; error?: boolean };

export default function App() {
  const [mode, setMode] = useState<Mode>(() => {
    const saved = localStorage.getItem(MODE_KEY);
    if (saved === "socks5" || saved === "advanced" || saved === "dashboard") return saved;
    return "dashboard";
  });
  const [profile, setProfile] = useState<ConnectionProfile>(DEFAULT_PROFILE);
  const [snapshot, setSnapshot] = useState<CoreSnapshot>(IDLE_SNAPSHOT);
  const [probe, setProbe] = useState<CoreProbe>({
    available: false, path: null, version: null, message: "Checking core…",
  });
  const [logs, setLogs] = useState<CoreLogEvent[]>([]);
  const [runtime, setRuntime] = useState("VPS server");
  const [toast, setToast] = useState<Toast | null>(null);
  const [palette, setPalette] = useState(false);
  const [jumpTo, setJumpTo] = useState<{ section: SectionId; at: number } | null>(null);

  const t = useT();
  const desktop = isDesktopRuntime();

  useEffect(() => {
    applyLanguage("zh");
  }, []);

  const effective = useMemo(() => withNormalizedEndpoint(profile), [profile]);

  const notify = useCallback((title: string, message: string, error?: boolean) => {
    setToast({ title, message, error });
  }, []);
  const showError = useCallback(
    (error: unknown) => notify(t("Action failed"), error instanceof Error ? error.message : String(error), true),
    [notify, t],
  );

  useEffect(() => {
    localStorage.setItem(MODE_KEY, mode);
  }, [mode]);

  // Bootstrap: subscribe to SSE events first, then load saved state
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;

    void (async () => {
      try {
        unsubscribe = await subscribeCore(
          (next) => { if (!disposed) setSnapshot(next); },
          (batch) => { if (!disposed) setLogs((current) => [...current, ...batch].slice(-1000)); },
        );
        if (disposed) return;

        const [info, saved, current, history] = await Promise.allSettled([
          runtimeInfo(), loadProfile(), getCoreStatus(), getCoreLogs(),
        ]);
        if (disposed) return;

        if (info.status === "fulfilled") setRuntime(`${info.value.os} · ${info.value.arch}`);
        if (saved.status === "fulfilled") setProfile(saved.value);
        if (current.status === "fulfilled") setSnapshot(current.value);
        if (history.status === "fulfilled") setLogs(history.value);
        if (saved.status === "rejected") showError(saved.reason);

        setProbe(await probeCore(
          saved.status === "fulfilled" ? saved.value : DEFAULT_PROFILE,
        ));
      } catch (error) {
        if (!disposed) showError(error);
      }
    })();

    return () => { disposed = true; unsubscribe?.(); };
  }, [desktop, showError]);

  // Reload profile when switched from profile selector or manager
  useEffect(() => {
    const onProfileSwitched = async () => {
      try {
        const saved = await loadProfile();
        setProfile(saved);
        setProbe(await probeCore(saved));
      } catch (e) {
        console.error("Failed to reload profile on switch", e);
      }
    };
    window.addEventListener("ProfileSwitched", onProfileSwitched);
    return () => window.removeEventListener("ProfileSwitched", onProfileSwitched);
  }, []);

  // Auto-dismiss toasts after 4 s
  useEffect(() => {
    if (!toast) return;
    const timeout = window.setTimeout(() => setToast(null), 4_000);
    return () => window.clearTimeout(timeout);
  }, [toast]);

  // Keyboard shortcuts (Ctrl+K for search)
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPalette(true);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const saveProfile = useCallback(async () => {
    try {
      if (!desktop) {
        localStorage.setItem("nextvpn-profile", JSON.stringify(profile));
      } else {
        const saved = await persistProfile(effective);
        setProfile((current) => ({
          ...saved,
          accessClientSecret: current.accessClientSecret,
          accessToken: current.accessToken,
        }));
      }
      notify(t("Profile saved"), t("Settings are stored locally on this device."));
    } catch (error) {
      showError(error);
    }
  }, [desktop, profile, effective, notify, showError, t]);

  return (
    <div className="flex h-full flex-col bg-background">
      {/* ── Header (Clean VPS Gateway Navigation) ──────────────────────── */}
      <header className="flex h-[56px] shrink-0 items-center justify-between border-b bg-[linear-gradient(180deg,hsl(var(--card-top)),hsl(var(--card)))] px-[18px] shadow-[inset_0_1px_0_hsl(0_0%_100%/0.03)]">
        <div className="flex items-center gap-2.5">
          <img src={logo} alt="" className="size-[26px] rounded-[7px]" />
          <span className="text-[14.5px] font-semibold tracking-tight">NextVPN</span>
          <span className="rounded bg-primary/10 px-2 py-0.5 text-[11px] font-medium text-primary border border-primary/20">
            VPS Gateway
          </span>
        </div>
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            onClick={() => setPalette(true)}
            className="flex h-8 items-center gap-1.5 rounded-lg border bg-muted px-2.5 text-[12px]
              text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none
              focus-visible:ring-2 focus-visible:ring-ring"
          >
            <Search className="size-[13px]" />
            <span>{t("Search settings")}</span>
            <kbd className="rounded border bg-secondary px-1.5 py-0.5 text-[10px] font-semibold text-foreground">
              Ctrl K
            </kbd>
          </button>
          <Tabs value={mode} onValueChange={(value) => setMode(value as Mode)}>
            <TabsList className="h-8">
              <TabsTrigger value="dashboard" className="px-3 py-1 text-[13px] flex items-center gap-1.5">
                <Activity className="size-3.5" />
                <span>运行概览</span>
              </TabsTrigger>
              <TabsTrigger value="socks5" className="px-3 py-1 text-[13px] flex items-center gap-1.5">
                <Server className="size-3.5" />
                <span>SOCKS5 管理</span>
              </TabsTrigger>
              <TabsTrigger value="advanced" className="px-3 py-1 text-[13px] flex items-center gap-1.5">
                <Settings2 className="size-3.5" />
                <span>底层设置</span>
              </TabsTrigger>
            </TabsList>
          </Tabs>
        </div>
      </header>

      {/* ── Main content ───────────────────────────────────────────────── */}
      <main className="min-h-0 flex-1 overflow-y-auto">
        {mode === "dashboard" ? (
          <SocksDashboard
            onManageClick={() => setMode("socks5")}
            onToast={notify}
          />
        ) : mode === "socks5" ? (
          <SocksManager
            onToast={notify}
            onViewDashboard={() => setMode("dashboard")}
          />
        ) : (
          <Advanced
            profile={profile}
            onChange={setProfile}
            snapshot={snapshot}
            probe={probe}
            logs={logs}
            runtime={runtime}
            appVersion={appVersion}
            onSave={() => void saveProfile()}
            onToast={notify}
            jumpTo={jumpTo}
          />
        )}
      </main>

      {/* ── Command palette ────────────────────────────────────────────── */}
      <CommandPalette
        open={palette}
        onClose={() => setPalette(false)}
        onPick={(section) => {
          setMode("advanced");
          setJumpTo({ section, at: Date.now() });
        }}
      />

      {/* ── Footer ─────────────────────────────────────────────────────── */}
      <Credits engineVersion={probe.version} runtime={runtime} />

      {/* ── Toast ──────────────────────────────────────────────────────── */}
      {toast ? (
        <div
          role="status"
          className={[
            "fixed bottom-5 end-5 z-50 flex max-w-[420px] items-start gap-2.5 rounded-lg border bg-popover p-3.5 shadow-lg",
            toast.error ? "border-destructive/50" : "border-border",
          ].join(" ")}
        >
          <BadgeCheck className={toast.error ? "size-4 text-destructive" : "size-4 text-primary"} />
          <div className="flex min-w-0 flex-col gap-0.5">
            <span className="text-[13px] font-semibold">{toast.title}</span>
            <span className="break-words text-xs text-muted-foreground">{toast.message}</span>
          </div>
          <button
            type="button"
            aria-label="Dismiss"
            className="ms-auto shrink-0 text-muted-foreground hover:text-foreground"
            onClick={() => setToast(null)}
          >
            <X className="size-3.5" />
          </button>
        </div>
      ) : null}
    </div>
  );
}

// ── Footer ────────────────────────────────────────────────────────────────────

function Credits({ engineVersion, runtime }: { engineVersion: string | null; runtime: string }) {
  return (
    <footer className="flex h-[34px] shrink-0 items-center justify-between border-t bg-sidebar-foot px-[18px] text-[11.5px] text-muted-foreground">
      <div className="flex min-w-0 items-center gap-2.5">
        <img src={logo} alt="" className="size-3.5 rounded opacity-85" />
        <span>
          NextVPN <b className="font-semibold text-foreground/70">{appVersion}</b>
        </span>
        <Rule />
        <span className="truncate">
          engine <span className="tabular font-mono">{engineVersion ?? "unavailable"}</span>
        </span>
        <Rule />
        <span className="truncate text-muted-foreground/70">{runtime}</span>
      </div>
      <span className="shrink-0 text-muted-foreground/60">Built on the Aether engine · AGPL-3.0</span>
    </footer>
  );
}

function Rule() {
  return <span className="text-border-strong">|</span>;
}
