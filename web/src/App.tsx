import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { BadgeCheck, Search, Settings2, ShieldAlert, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Advanced } from "@/features/Advanced";
import { Simple } from "@/features/Simple";
import { type CarryMode, carryFromProfile } from "@/features/carry";
import { SAMPLE_MS, type Sample, append } from "@/features/latency";
import { CommandPalette } from "@/features/CommandPalette";
import type { SectionId } from "@/features/settingsIndex";
import logo from "@/assets/logo.png";
import { applyLanguage } from "@/core/i18n";
import { useT } from "@/core/useT";
import {
  getCoreLogs, getCoreStatus, isDesktopRuntime, loadProfile, probeCore, probeLatency, runtimeInfo,
  saveProfile as persistProfile, setFullTunnel, setSystemProxy, startCore, stopCore, subscribeCore,
} from "@/core/api";
import { withNormalizedEndpoint } from "@/core/endpoint";
import {
  DEFAULT_PROFILE, IDLE_SNAPSHOT, type ConnectionProfile, type CoreLogEvent, type CoreProbe,
  type CoreSnapshot,
} from "@/types";

// NextVPN runs as a headless VPS server — no tray icons, no OS UAC elevation.
// This flag gates every feature that only makes sense on a desktop installation.
const IS_VPS_MODE = true;

const ACTIVE = new Set(["starting", "scanning", "connecting", "connected", "reconnecting"]);
const MODE_KEY = "nextvpn.mode";
const appVersion = import.meta.env.VITE_APP_VERSION || "unknown";

type Mode = "simple" | "advanced";
type Toast = { title: string; message: string; error?: boolean };

export default function App() {
  const [mode, setMode] = useState<Mode>(
    () => (localStorage.getItem(MODE_KEY) as Mode | null) ?? "simple",
  );
  const [profile, setProfile] = useState<ConnectionProfile>(DEFAULT_PROFILE);
  const [snapshot, setSnapshot] = useState<CoreSnapshot>(IDLE_SNAPSHOT);
  const [probe, setProbe] = useState<CoreProbe>({
    available: false, path: null, version: null, message: "Checking core…",
  });
  const [logs, setLogs] = useState<CoreLogEvent[]>([]);
  const [runtime, setRuntime] = useState("VPS server");
  const [toast, setToast] = useState<Toast | null>(null);
  const [latency, setLatency] = useState<Sample[]>([]);
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

  // Latency chart: sample every SAMPLE_MS while connected
  useEffect(() => {
    if (!desktop || snapshot.state !== "connected") {
      setLatency([]);
      return;
    }
    let disposed = false;
    let timer = 0;

    const sample = async () => {
      try {
        const value = await probeLatency();
        if (!disposed) setLatency((history) => append(history, value));
      } catch {
        if (!disposed) setLatency((history) => append(history, null));
      }
      if (!disposed) timer = window.setTimeout(() => void sample(), SAMPLE_MS);
    };
    void sample();
    return () => { disposed = true; window.clearTimeout(timer); };
  }, [desktop, snapshot.state]);

  // Toggle connection
  const toggleConnection = useCallback(async () => {
    try {
      if (ACTIVE.has(snapshot.state)) {
        setSnapshot(await stopCore());
        notify(t("Disconnected"), t("The tunnel stopped cleanly."));
        return;
      }
      const latest = await probeCore(effective);
      setProbe(latest);
      if (!latest.available) throw new Error(latest.message);
      setSnapshot(await startCore(effective));
    } catch (error) {
      showError(error);
    }
  }, [snapshot.state, effective, notify, showError, t]);

  const toggleRef = useRef(toggleConnection);
  useEffect(() => { toggleRef.current = toggleConnection; }, [toggleConnection]);

  // Keyboard shortcuts
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
        event.preventDefault();
        void toggleRef.current();
      }
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

  const applyProfile = useCallback(
    (patch: Partial<ConnectionProfile>) => {
      setProfile((current) => {
        const next = { ...current, ...patch };
        if (desktop) {
          void persistProfile(withNormalizedEndpoint(next)).catch(showError);
        }
        return next;
      });
    },
    [desktop, showError],
  );

  const carry: CarryMode = carryFromProfile(profile.systemProxy, profile.fullTunnel);

  const setCarry = useCallback(
    (next: CarryMode) => {
      const wantsSystem = next === "system";
      // Full tunnel (TUN device) is not supported in VPS / headless mode.
      // Silently fall back to system proxy when someone selects it, and
      // notify so the choice is not just lost.
      if (next === "tun" && IS_VPS_MODE) {
        notify(
          t("Full tunnel not available"),
          t("TUN device capture is not supported in VPS mode. Using system proxy instead."),
          true,
        );
        applyProfile({ systemProxy: true, fullTunnel: false });
        if (ACTIVE.has(snapshot.state)) {
          void setSystemProxy(true).catch(showError);
        }
        return;
      }

      applyProfile({ systemProxy: wantsSystem, fullTunnel: false });
      if (!desktop || !ACTIVE.has(snapshot.state)) return;

      void setFullTunnel(false)
        .then(() => setSystemProxy(wantsSystem))
        .then((applied) => {
          if (wantsSystem && applied) {
            notify(t("System proxy set"), t("Your system proxy now follows the active route."));
          } else if (next === "app") {
            notify(t("Proxy cleared"), t("Your system proxy has been put back."));
          }
        })
        .catch(showError);
    },
    [applyProfile, desktop, snapshot.state, notify, showError, t],
  );

  const retryStealth = useCallback(async () => {
    const next: ConnectionProfile = { ...profile, scanMode: "stealth" };
    setProfile(next);
    try {
      setSnapshot(await startCore(withNormalizedEndpoint(next)));
    } catch (error) {
      showError(error);
    }
  }, [profile, showError]);

  return (
    <div className="flex h-full flex-col bg-background">
      {/* ── Header ─────────────────────────────────────────────────────── */}
      <header className="flex h-[56px] shrink-0 items-center justify-between border-b bg-[linear-gradient(180deg,hsl(var(--card-top)),hsl(var(--card)))] px-[18px] shadow-[inset_0_1px_0_hsl(0_0%_100%/0.03)]">
        <div className="flex items-center gap-2.5">
          <img src={logo} alt="" className="size-[26px] rounded-[7px]" />
          <span className="text-[14.5px] font-semibold tracking-tight">NextVPN</span>
          <StateChip snapshot={snapshot} />
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
              <TabsTrigger value="simple" className="px-3 py-1 text-[13px]">{t("Simple")}</TabsTrigger>
              <TabsTrigger value="advanced" className="px-3 py-1 text-[13px]">{t("Advanced")}</TabsTrigger>
            </TabsList>
          </Tabs>
          <Button variant="ghost" size="icon" aria-label={t("Advanced settings")} onClick={() => setMode("advanced")}>
            <Settings2 />
          </Button>
        </div>
      </header>

      {/* ── Kill-switch warning banner ─────────────────────────────────── */}
      {snapshot.blocking ? (
        <div className="flex shrink-0 items-center gap-3 border-b border-warning/30 bg-warning/[0.09] px-[18px] py-2.5">
          <ShieldAlert className="size-4 shrink-0 text-warning" />
          <div className="min-w-0 flex-1">
            <div className="text-[12.5px] font-semibold text-warning">{t("Traffic is blocked, not broken")}</div>
            <div className="truncate text-[11.5px] text-muted-foreground">
              {snapshot.statusMessage ??
                t("The tunnel is down and your system proxy still points at it, so nothing leaves in the clear.")}
            </div>
          </div>
          <Button
            size="sm"
            variant="outline"
            className="shrink-0"
            onClick={async () => {
              try {
                setSnapshot(await stopCore());
                notify(t("Connection restored"), t("Your system proxy has been put back."));
              } catch (error) {
                showError(error);
              }
            }}
          >
            {t("Restore my connection")}
          </Button>
        </div>
      ) : null}

      {/* ── Main content ───────────────────────────────────────────────── */}
      <main className="min-h-0 flex-1">
        {mode === "simple" ? (
          <Simple
            snapshot={snapshot}
            profile={profile}
            probe={probe}
            carry={carry}
            latency={latency}
            onCarry={setCarry}
            onToggle={() => void toggleConnection()}
            onAdvanced={(section) => {
              setMode("advanced");
              if (section) setJumpTo({ section: section as SectionId, at: Date.now() });
            }}
            onRetryStealth={() => void retryStealth()}
            onReport={() => setMode("advanced")}
            onProfile={applyProfile}
            onToast={notify}
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

// ── State chip ────────────────────────────────────────────────────────────────

function StateChip({ snapshot }: { snapshot: CoreSnapshot }) {
  if (snapshot.state === "connected")
    return (
      <span className="ms-1.5 inline-flex h-[22px] items-center gap-1.5 rounded-full border border-primary/30 bg-primary/[0.13] px-2.5 text-[11.5px] font-semibold text-primary">
        <span className="size-1.5 rounded-full bg-current" />
        Connected
      </span>
    );
  if (snapshot.state === "error")
    return (
      <span className="ms-1.5 inline-flex h-[22px] items-center gap-1.5 rounded-full border border-destructive/40 bg-destructive/10 px-2.5 text-[11.5px] font-semibold text-destructive">
        <span className="size-1.5 rounded-full bg-current" />
        Stopped
      </span>
    );
  if (!ACTIVE.has(snapshot.state))
    return (
      <span className="ms-1.5 inline-flex h-[22px] items-center gap-1.5 rounded-full border bg-muted px-2.5 text-[11.5px] font-semibold text-muted-foreground">
        <span className="size-1.5 rounded-full bg-current" />
        Not connected
      </span>
    );
  return (
    <span className="ms-1.5 inline-flex h-[22px] items-center gap-1.5 rounded-full border border-warning/30 bg-warning/[0.13] px-2.5 text-[11.5px] font-semibold text-warning">
      <span className="size-1.5 animate-pulse rounded-full bg-current" />
      {snapshot.attempt > 0 ? `Searching · ${snapshot.attempt} of ${snapshot.maxAttempts}` : "Searching"}
    </span>
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
