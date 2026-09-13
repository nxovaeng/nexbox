import { useCallback, useEffect, useRef, useState } from "react";
import { useT } from "@/core/useT";
import { CheckCircle2, Loader2, Radar, Search, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { cancelScan, scanEndpoints, testEndpoint, type ScanCandidate } from "@/core/api";
import { normalizeEndpoint } from "@/core/endpoint";
import { byNetwork, summarise } from "./grouping";
import type { ConnectionProfile, CoreSnapshot } from "@/types";

type Phase = "idle" | "scanning" | "testing" | "cancelling";

/**
 * The core's own ceiling. Asking for fewer hides whole ranges: results are
 * ranked by round-trip time, so the nearest network fills the list and the
 * alternatives you would want when it is throttled never appear.
 */
const SCAN_LIMIT = 16;

interface ScannerProps {
  profile: ConnectionProfile;
  snapshot: CoreSnapshot;
  onPick: (endpoint: string) => void;
  onToast: (title: string, message: string, error?: boolean) => void;
}

export function Scanner({ profile, snapshot, onPick, onToast }: ScannerProps) {
  const t = useT();
  const [phase, setPhase] = useState<Phase>("idle");
  const [candidates, setCandidates] = useState<ScanCandidate[]>([]);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // A scan outlives this panel if the user navigates away mid-search, so the
  // late result must not be written into an unmounted component.
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => { live.current = false; };
  }, []);

  const busy = phase === "scanning" || phase === "testing";
  // The core's reporting modes wrap a MASQUE-only probe, so the picker under
  // Routes has no effect here. Saying so beats letting someone select WireGuard
  // and conclude the scanner is broken.
  const masqueOnly = profile.protocol !== "masque";
  const connected = snapshot.state !== "idle" && snapshot.state !== "stopped" && snapshot.state !== "error";
  const pinned = normalizeEndpoint(profile.peer ?? "");

  const scan = useCallback(async () => {
    setPhase("scanning");
    setError(null);
    setNote(null);
    try {
      const outcome = await scanEndpoints(profile, SCAN_LIMIT);
      if (!live.current) return;
      const got = Array.isArray(outcome.candidates) ? outcome.candidates : [];
      setCandidates(got);
      if (got.length === 0) {
        setNote(t("Nothing answered on either transport. This network is filtering hard."));
      } else {
        setNote(
          outcome.fellBack
            ? `Nothing over ${label(profile.masqueTransport)}; these answered over ${label(outcome.transport)}.`
            : summarise(got),
        );
      }
    } catch (raw) {
      if (!live.current) return;
      const message = raw instanceof Error ? raw.message : String(raw);
      // Cancelling is a normal outcome, not a failure to shout about.
      if (message.includes("cancelled")) setNote("Scan cancelled.");
      else setError(message);
    } finally {
      if (live.current) setPhase("idle");
    }
  }, [profile]);

  const stop = useCallback(async () => {
    setPhase("cancelling");
    try {
      await cancelScan();
    } catch {
      /* the scan finished on its own between the click and the call */
    }
  }, []);

  const test = useCallback(async () => {
    const address = normalizeEndpoint(profile.peer ?? "");
    if (!address) {
      setError(t("Enter a numeric address and port first."));
      return;
    }
    setPhase("testing");
    setError(null);
    setNote(null);
    try {
      const result = await testEndpoint(profile, address);
      if (!live.current) return;
      setNote(`${result.peer} answered in ${result.rttMs} ms.`);
    } catch (raw) {
      if (!live.current) return;
      setError(raw instanceof Error ? raw.message : String(raw));
    } finally {
      if (live.current) setPhase("idle");
    }
  }, [profile]);

  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between gap-4 space-y-0 pb-3">
        <div className="flex flex-col gap-1.5">
          <CardTitle className="text-[15px]">{t("Find a gateway")}</CardTitle>
          <CardDescription>
            {/* Split around the transport name so the two halves can be ordered
                the way each language orders them. */}
            {t("Tests real MASQUE gateways over")} {label(profile.masqueTransport)}{" "}
            {t("and ranks them by round-trip time. Nothing is connected until you pick one.")}
          </CardDescription>
        </div>
        <div className="flex shrink-0 gap-2">
          {phase === "scanning" ? (
            <Button variant="outline" size="sm" onClick={() => void stop()}>
              <X />
              {t("Stop")}
            </Button>
          ) : (
            <Button size="sm" disabled={connected || busy || phase === "cancelling"} onClick={() => void scan()}>
              {phase === "cancelling" ? <Loader2 className="animate-spin" /> : <Radar />}
              {t("Scan")}
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            disabled={connected || busy || !pinned}
            onClick={() => void test()}
          >
            {phase === "testing" ? <Loader2 className="animate-spin" /> : <Search />}
            {t("Test pinned")}
          </Button>
        </div>
      </CardHeader>

      <CardContent className="pt-0">
        {masqueOnly ? (
          <p className="py-2 text-[13px] text-muted-foreground">
            Your protocol is set to{" "}
            <span className="font-medium text-foreground">
              {profile.protocol === "wg" ? "WireGuard" : "WARP in WARP"}
            </span>
            . This searches for MASQUE gateways only, so anything found here applies when you switch back to
            MASQUE — it will not change how {profile.protocol === "wg" ? "WireGuard" : "WARP in WARP"} connects.
          </p>
        ) : null}

        {connected ? (
          <p className="py-2 text-[13px] text-muted-foreground">
            Disconnect first — scanning while connected competes with the tunnel for the same gateways and
            reports worse numbers than the network really offers.
          </p>
        ) : null}

        {phase === "scanning" ? (
          <div className="flex items-center gap-2.5 py-3 text-[13px] text-muted-foreground">
            <Loader2 className="size-4 animate-spin text-primary" />
            Testing gateways over {label(profile.masqueTransport)}. This takes a while on a filtered network.
          </div>
        ) : null}

        {error ? <p className="py-2 text-[13px] text-destructive">{error}</p> : null}
        {note && !error ? <p className="py-2 text-[13px] text-muted-foreground">{note}</p> : null}

        {candidates.length > 0 ? (
          <div className="mt-1 flex flex-col gap-2.5">
            {byNetwork(candidates).map(({ network, members }) => (
              <div key={network} className="overflow-hidden rounded-md border">
                <div className="flex items-baseline justify-between gap-3 border-b bg-muted/40 px-3.5 py-2">
                  <span className="font-mono text-[12px] font-medium">{network}</span>
                  <span className="text-[11.5px] text-muted-foreground">
                    {members.length} gateway{members.length === 1 ? "" : "s"} · best{" "}
                    <span className="tabular font-mono">{members[0].rttMs} ms</span>
                  </span>
                </div>
                {members.map((candidate) => {
                  const chosen = pinned === candidate.peer;
                  const rank = candidates.indexOf(candidate) + 1;
                  return (
                    <button
                      key={candidate.peer}
                      type="button"
                      onClick={() => {
                        onPick(candidate.peer);
                        onToast("Endpoint pinned", `${candidate.peer} — set Endpoint mode to use it.`);
                      }}
                      className={[
                        "flex w-full items-center justify-between gap-3 border-b px-3.5 py-2.5 text-start transition-colors last:border-b-0",
                        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                        chosen ? "bg-primary/10" : "hover:bg-accent",
                      ].join(" ")}
                    >
                      <div className="flex min-w-0 items-center gap-3">
                        <span className="tabular w-5 shrink-0 font-mono text-[11px] text-muted-foreground">
                          {rank}
                        </span>
                        <span className="truncate font-mono text-[13px]">{candidate.peer}</span>
                        {chosen ? <CheckCircle2 className="size-4 shrink-0 text-primary" /> : null}
                      </div>
                      <div className="flex shrink-0 items-center gap-2.5">
                        <Latency ms={candidate.rttMs} best={candidates[0].rttMs} />
                        <span className="tabular w-16 text-end font-mono text-[13px]">
                          {candidate.rttMs} ms
                        </span>
                      </div>
                    </button>
                  );
                })}
              </div>
            ))}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

/** Relative bar, so the spread between candidates reads without doing the arithmetic. */
function Latency({ ms, best }: { ms: number; best: number }) {
  const ratio = Math.min(1, best > 0 ? best / Math.max(ms, 1) : 1);
  const tone = ms < 120 ? "bg-primary" : ms < 300 ? "bg-warning" : "bg-destructive";
  return (
    <span className="hidden h-1.5 w-20 overflow-hidden rounded-full bg-secondary sm:block">
      <span className={`block h-full rounded-full ${tone}`} style={{ width: `${Math.max(8, ratio * 100)}%` }} />
    </span>
  );
}

function label(transport: string): string {
  return transport === "h2" ? "MASQUE H2" : "MASQUE H3";
}

// ── WarpScout scanner (embedded in the Endpoint section) ─────────────────────

import { Globe } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Separator } from "@/components/ui/separator";
import {
  type WarpEndpoint, type WarpScanOptions, type WarpScoutStatus,
  warpscoutStatus, warpscoutInstall, warpscoutRegister, warpscoutBridgeAccount,
  warpscoutScan,
} from "@/core/api";

const WS_PROTOCOLS: Array<{ id: WarpScanOptions["protocol"]; label: string }> = [
  { id: "masque-h2", label: "MASQUE H2" },
  { id: "masque",    label: "MASQUE H3" },
  { id: "awg",       label: "AmneziaWG" },
  { id: "wg",        label: "WireGuard" },
];

interface WarpScoutScannerProps {
  /** Called with the selected ip:port so the parent can fill the peer field */
  onPick: (endpoint: string) => void;
  onToast: (title: string, message: string, error?: boolean) => void;
}

export function WarpScoutScanner({ onPick, onToast }: WarpScoutScannerProps) {
  const t = useT();
  const [ws, setWs] = useState<WarpScoutStatus | null>(null);
  const [installing, setInstalling] = useState(false);
  const [registering, setRegistering] = useState(false);

  const [scanning, setScanning] = useState(false);
  const [results, setResults] = useState<WarpEndpoint[] | null>(null);
  const [scanRaw, setScanRaw] = useState("");
  const [scanError, setScanError] = useState<string | null>(null);

  const [protocol, setProtocol] = useState<WarpScanOptions["protocol"]>("masque-h2");
  const [country, setCountry] = useState("");
  const [excludeNode, setExcludeNode] = useState("");
  const [sample, setSample] = useState(5);
  const [tunPing, setTunPing] = useState(false);

  useEffect(() => {
    warpscoutStatus()
      .then(setWs)
      .catch(() => {});
  }, []);

  const install = async () => {
    setInstalling(true);
    try {
      setWs(await warpscoutInstall());
      onToast("WarpScout", "Installed successfully.");
    } catch (e: unknown) {
      onToast("Install failed", e instanceof Error ? e.message : String(e), true);
    } finally {
      setInstalling(false);
    }
  };

  const registerAccount = async (bridge: boolean) => {
    if (!ws) return;
    setRegistering(true);
    try {
      const next = bridge ? await warpscoutBridgeAccount() : await warpscoutRegister();
      setWs(next);
      if (next.accountReady) {
        onToast("Account ready", "Ready to scan for endpoints.");
      }
    } catch (err: unknown) {
      onToast("Registration failed", err instanceof Error ? err.message : String(err));
    } finally {
      setRegistering(false);
    }
  };

  const scan = async () => {
    setScanning(true);
    setScanError(null);
    setResults(null);
    try {
      const r = await warpscoutScan({
        protocol,
        country: country.trim() || undefined,
        excludeNode: excludeNode.trim() || undefined,
        sample,
        tunPing,
      });
      setResults(r.endpoints);
      setScanRaw(r.raw);
      if (r.endpoints.length === 0) {
        setScanError(t("Scan finished but no usable endpoints were found."));
      }
    } catch (e: unknown) {
      setScanError(e instanceof Error ? e.message : String(e));
    } finally {
      setScanning(false);
    }
  };

  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between gap-4 space-y-0 pb-3">
        <div className="flex flex-col gap-1.5">
          <CardTitle className="text-[15px]">WarpScout</CardTitle>
          <CardDescription>
            {t("Find the best WARP / MASQUE endpoints by country and node. Click a result to use it as the pinned endpoint.")}
          </CardDescription>
        </div>

        {/* Install / status badge */}
        {!ws?.installed ? (
          <Button size="sm" disabled={installing} onClick={() => void install()}>
            {installing ? t("Downloading…") : t("Install WarpScout")}
          </Button>
        ) : (
          <span className="flex items-center gap-1 shrink-0 text-[12.5px] text-primary">
            <CheckCircle2 className="size-4" />
            {ws.version ?? "installed"}
          </span>
        )}
      </CardHeader>

      {ws?.installed && (
        <CardContent className="flex flex-col gap-4 pt-0">
          {/* Account */}
          {!ws.accountReady ? (
            <div className="flex flex-col gap-2 rounded-lg border bg-muted/30 px-3 py-2.5">
              <div className="flex items-center justify-between gap-4">
                <span className="text-[13px] text-muted-foreground">
                  {ws.accountSource === "aetherIdentity" 
                    ? t("Aether identity found — use it, or register a new one") 
                    : t("No account — register a WARP account first")}
                </span>
                <Button size="sm" disabled={registering} onClick={() => void registerAccount(false)}>
                  {registering ? t("Registering…") : t("Register new")}
                </Button>
              </div>
              {ws.accountSource === "aetherIdentity" && (
                <div className="flex items-center justify-between gap-4 border-t pt-2 mt-1">
                  <span className="text-[12px] text-muted-foreground">
                    {t("Copy aether's credentials to scan with")}
                  </span>
                  <Button size="sm" variant="secondary" disabled={registering} onClick={() => void registerAccount(true)}>
                    {registering ? t("…") : t("Use aether identity")}
                  </Button>
                </div>
              )}
            </div>
          ) : (
            <div className="flex items-center justify-between gap-2 text-[12.5px] text-muted-foreground">
              <span>
                {ws.accountSource === "aetherIdentity" 
                  ? t("Account: bridged from aether") 
                  : t("Account: warpscout registration")}
              </span>
              <div className="flex items-center gap-2">
                <Button size="sm" variant="ghost" disabled={registering} onClick={() => void registerAccount(true)}>
                  {registering ? t("…") : t("Bridge")}
                </Button>
                <Button size="sm" variant="ghost" disabled={registering} onClick={() => void registerAccount(false)}>
                  {registering ? t("…") : t("Register")}
                </Button>
              </div>
            </div>
          )}

          {ws.accountReady && (
            <>
              <Separator />

              {/* Scan options */}
              <div className="grid grid-cols-2 gap-4">
                {/* Protocol */}
                <div className="flex flex-col gap-2">
                  <Label className="text-[13px]">{t("Protocol")}</Label>
                  <div className="grid grid-cols-2 gap-1.5">
                    {WS_PROTOCOLS.map((p) => (
                      <button
                        key={p.id}
                        type="button"
                        aria-pressed={protocol === p.id}
                        onClick={() => setProtocol(p.id)}
                        className={[
                          "rounded-md border px-2.5 py-1.5 text-[12.5px] text-start transition-colors",
                          protocol === p.id
                            ? "border-primary bg-primary/10 font-semibold text-primary"
                            : "border-border hover:bg-accent",
                        ].join(" ")}
                      >
                        {p.label}
                      </button>
                    ))}
                  </div>
                </div>

                {/* Filters */}
                <div className="flex flex-col gap-2.5">
                  <div className="flex flex-col gap-1">
                    <Label className="text-[12.5px] text-muted-foreground">
                      <Globe className="mr-1 inline size-3" />
                      {t("Exit country")}
                      <span className="ml-1 text-[11px]">{t("e.g. DE,NL — blank = any")}</span>
                    </Label>
                    <Input
                      className="h-8 font-mono text-[13px] uppercase"
                      placeholder="DE,NL,JP"
                      value={country}
                      onChange={(e) => setCountry(e.target.value.toUpperCase())}
                    />
                  </div>
                  <div className="flex flex-col gap-1">
                    <Label className="text-[12.5px] text-muted-foreground">
                      {t("Exclude node")}
                      <span className="ml-1 text-[11px]">{t("e.g. DME")}</span>
                    </Label>
                    <Input
                      className="h-8 font-mono text-[13px] uppercase"
                      placeholder="DME"
                      value={excludeNode}
                      onChange={(e) => setExcludeNode(e.target.value.toUpperCase())}
                    />
                  </div>
                </div>
              </div>

              {/* Sample + tun-ping */}
              <div className="flex flex-wrap items-center gap-5">
                <div className="flex items-center gap-2">
                  <Label className="text-[12.5px] text-muted-foreground">{t("Addresses / subnet")}</Label>
                  <Input
                    type="number" min={1} max={20}
                    className="h-8 w-16 text-center font-mono text-[13px]"
                    value={sample}
                    onChange={(e) => setSample(Math.max(1, Math.min(20, Number(e.target.value))))}
                  />
                </div>
                <div className="flex items-center gap-2">
                  <Switch checked={tunPing} onCheckedChange={setTunPing} />
                  <Label className="text-[12.5px] text-muted-foreground">
                    {t("In-tunnel ping")}
                    <span className="ml-1 text-[11px]">{t("(slower, weeds out torn-down endpoints)")}</span>
                  </Label>
                </div>
              </div>

              <Button
                size="sm"
                className="self-start"
                disabled={scanning}
                onClick={() => void scan()}
              >
                <Radar className={scanning ? "animate-spin" : ""} />
                {scanning ? t("Scanning…") : t("Scan endpoints")}
              </Button>

              {/* Results */}
              {scanError && (
                <p className="text-[12.5px] text-destructive">{scanError}</p>
              )}

              {results !== null && results.length > 0 && (
                <div className="flex flex-col gap-1.5">
                  <p className="text-[12px] text-muted-foreground">
                    {results.length} {t("endpoint(s) — click to pin")}
                  </p>
                  <div className="overflow-hidden rounded-md border">
                    <div className="grid grid-cols-[1fr_52px_52px_1fr_60px] gap-2 border-b bg-muted/40 px-3 py-1.5 font-mono text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
                      <span>Endpoint</span>
                      <span>CC</span>
                      <span>Node</span>
                      <span>Location</span>
                      <span className="text-end">Ping</span>
                    </div>
                    {results.map((ep, i) => (
                      <button
                        key={`${ep.endpoint}-${i}`}
                        type="button"
                        title={t("Click to pin this endpoint")}
                        onClick={() => {
                          onPick(ep.endpoint);
                          onToast("Endpoint pinned", `${ep.endpoint} (${ep.node}, ${ep.country})`);
                        }}
                        className="grid w-full grid-cols-[1fr_52px_52px_1fr_60px] gap-2 border-b px-3 py-2 text-start transition-colors last:border-b-0 hover:bg-primary/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      >
                        <span className="truncate font-mono text-[12.5px] font-medium">{ep.endpoint}</span>
                        <span className="text-[12px]">{ep.country}</span>
                        <span className="text-[12px] text-muted-foreground">{ep.node}</span>
                        <span className="truncate text-[11.5px] text-muted-foreground">{ep.nodeLocation}</span>
                        <span className="text-end tabular font-mono text-[12.5px] text-primary">
                          {ep.pingMs != null ? `${ep.pingMs}ms` : "—"}
                        </span>
                      </button>
                    ))}
                  </div>
                  <p className="text-[11.5px] text-muted-foreground">
                    {t("Set Endpoint mode to")} <b>{t("Custom first")}</b> {t("to use the pinned address.")}
                  </p>
                </div>
              )}

              {/* Raw output (diagnostic) */}
              {scanError && scanRaw && (
                <details className="text-[12px]">
                  <summary className="cursor-pointer select-none text-muted-foreground">{t("Raw output")}</summary>
                  <pre className="mt-2 max-h-[140px] overflow-auto whitespace-pre-wrap break-all rounded-md border bg-muted/40 p-2 font-mono text-[11px] text-muted-foreground">
                    {scanRaw}
                  </pre>
                </details>
              )}
            </>
          )}
        </CardContent>
      )}
    </Card>
  );
}
