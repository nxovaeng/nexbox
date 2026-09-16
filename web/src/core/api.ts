import type {
  CarrierKind,
  ChainSettings,
  ConnectionProfile,
  LanSettings,
  CoreLogEvent,
  CoreProbe,
  CoreSnapshot,
  PsiphonSnapshot,
  PsiphonInfo,
  TorSnapshot,
  RoutingRulesInfo,
  ProtonInfo,
  ProtonSettings,
  ProtonServerSummary,
  WindscribeStatusResponse,
  WindscribeAccount,
  WindscribeSettings,
  WindscribeServer,
  WindscribeSnapshot,
  SocksInstanceConfig,
  SocksInstanceStatus,
  SocksInstanceView,
  ConnectivityResult,
  SocksSpeedResult,
} from "../types";

export type {
  PsiphonInfo,
  RoutingRulesInfo,
  ProtonInfo,
  ProtonSettings,
  ProtonServerSummary,
  WindscribeStatusResponse,
  WindscribeAccount,
  WindscribeSettings,
  WindscribeServer,
  WindscribeSnapshot,
  SocksInstanceConfig,
  SocksInstanceStatus,
  SocksInstanceView,
  ConnectivityResult,
  SocksSpeedResult,
};

// Same-origin: frontend is embedded in the Rust binary, served from the same port
export const API_BASE = "/api";

export async function apiInvoke<T>(command: string, args?: any): Promise<T> {
  const res = await fetch(`${API_BASE}/${command}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: args !== undefined ? JSON.stringify(args) : "{}",
  });
  // Parse body first so we can inspect it regardless of status code.
  const text = await res.text();
  let body: unknown;
  try { body = JSON.parse(text); } catch { body = text; }

  // HTTP-level error
  if (!res.ok) throw body;

  // The backend returns HTTP 200 for application-level errors too, signalled by
  // an "error" key in the JSON body.  Treat those as thrown so callers don't
  // have to guard every field access against undefined.
  if (
    body !== null &&
    typeof body === "object" &&
    "error" in (body as object) &&
    (body as Record<string, unknown>)["error"] !== undefined &&
    (body as Record<string, unknown>)["error"] !== null
  ) {
    const msg = (body as Record<string, unknown>)["error"];
    throw new Error(typeof msg === "string" ? msg : JSON.stringify(msg));
  }

  return body as T;
}

export function isDesktopRuntime(): boolean {
  // Always true in headless API mode so the browser UI acts as the main controller
  return true;
}

export async function runtimeInfo(): Promise<{ os: string; arch: string }> {
  return apiInvoke("runtime_info");
}

export async function probeCore(profile: ConnectionProfile): Promise<CoreProbe> {
  return apiInvoke("probe_core", { profile });
}

export async function startCore(profile: ConnectionProfile): Promise<CoreSnapshot> {
  return apiInvoke("start_core", { profile });
}

export async function stopCore(): Promise<CoreSnapshot> {
  return apiInvoke("stop_core");
}

export async function getCoreStatus(): Promise<CoreSnapshot> {
  return apiInvoke("core_status");
}

export async function getCoreLogs(): Promise<CoreLogEvent[]> {
  return apiInvoke("core_logs");
}

export async function loadProfile(): Promise<ConnectionProfile> {
  return apiInvoke("load_profile");
}

export async function saveProfile(profile: ConnectionProfile): Promise<ConnectionProfile> {
  return apiInvoke("save_profile", { profile });
}

export async function saveReport(contents: string, filename: string): Promise<string> {
  return apiInvoke("save_report", { contents, filename });
}

export type UnlistenFn = () => void;

export async function subscribeCore(
  onStatus: (status: CoreSnapshot) => void,
  onLogs: (logs: CoreLogEvent[]) => void,
): Promise<UnlistenFn> {
  const sse = new EventSource(`${API_BASE}/events`);
  sse.addEventListener("core-status", (event) => {
    try {
      onStatus(JSON.parse(event.data));
    } catch (e) {
      console.error(e);
    }
  });
  sse.addEventListener("core-logs", (event) => {
    try {
      onLogs(JSON.parse(event.data));
    } catch (e) {
      console.error(e);
    }
  });
  sse.addEventListener("ProfileSwitched", (event) => {
    window.dispatchEvent(new CustomEvent("ProfileSwitched", { detail: event.data }));
  });
  return () => { sse.close(); };
}

export type TrayAction = "toggle-connection" | "open-diagnostics" | "restore-proxy";

export async function subscribeTrayActions(_onAction: (action: TrayAction) => void): Promise<UnlistenFn> {
  // Headless mode does not support tray icons.
  return () => { };
}

export interface ScanCandidate {
  peer: string;
  rttMs: number;
}

export interface ScanOutcome {
  candidates: ScanCandidate[];
  transport: string;
  fellBack: boolean;
}

export async function scanEndpoints(profile: ConnectionProfile, limit = 8): Promise<ScanOutcome> {
  return apiInvoke("scan_endpoints", { profile, limit });
}

export async function testEndpoint(profile: ConnectionProfile, endpoint: string): Promise<ScanCandidate> {
  return apiInvoke("test_endpoint", { profile, endpoint });
}

export async function cancelScan(): Promise<boolean> {
  return apiInvoke("cancel_scan");
}

export async function probeLatency(): Promise<number | null> {
  return apiInvoke("probe_latency");
}

export interface SpeedResult {
  mbps: number;
  bytes: number;
  seconds: number;
}

export async function speedTest(): Promise<SpeedResult> {
  return apiInvoke("speed_test");
}

export interface ExitInfo {
  ip: string;
  country: string;
  colo: string;
  warp: boolean;
  gateway: boolean;
  chained: boolean;
}

export async function exitInfo(): Promise<ExitInfo> {
  return apiInvoke("exit_info");
}

export interface ChainStatus {
  running: boolean;
  address: string | null;
}

export interface WarpScanResult {
  accounts: Array<{
    id: string;
    license: string;
    level: string;
    premiumData: number;
    quota: number;
    registeredOn: string | null;
  }>;
}

export interface ProfileSummary {
  id: string;
  name: string;
  protocol?: string;
  masqueTransport?: string;
  endpointMode?: string;
  peer?: string | null;
  dns?: string[];
  socksAddress?: string;
  upstreamProxy?: string;
  noize?: string;
  fragmentClientHello?: boolean;
}

export interface AetherProfileConfig {
  id: string;
  name: string;
  description?: string | null;
  protocol: "masque" | "wg" | "gool";
  masqueTransport: "h2" | "h3";
  scanMode: "turbo" | "balanced" | "thorough" | "stealth" | "ironclad";
  ipFamily: "both" | "v4" | "v6";
  socksAddress: string;
  noize: "off" | "light" | "balanced" | "firewall" | "gfw" | "aggressive";
  fragmentClientHello: boolean;
  fragmentSize: string;
  fragmentDelay: string;
  endpointMode: "automatic" | "custom-first" | "custom-only";
  peer?: string | null;
  dns: string[];
  keepaliveSecs: number;
  quickReconnect: boolean;
  dataCheck: boolean;
  logLevel: string;
  isActive: boolean;
}

export async function getAetherProfiles(): Promise<AetherProfileConfig[]> {
  const res = await fetch(`${API_BASE}/aether/profiles`);
  const text = await res.text();
  try {
    return JSON.parse(text) as AetherProfileConfig[];
  } catch {
    throw new Error(text);
  }
}

export async function getAetherProfile(id?: string): Promise<AetherProfileConfig> {
  const url = id ? `${API_BASE}/aether/profile?id=${encodeURIComponent(id)}` : `${API_BASE}/aether/profile`;
  const res = await fetch(url);
  const text = await res.text();
  try {
    return JSON.parse(text) as AetherProfileConfig;
  } catch {
    throw new Error(text);
  }
}

export async function saveAetherProfile(profile: AetherProfileConfig): Promise<AetherProfileConfig> {
  return apiInvoke("aether/profile", profile);
}

export async function duplicateAetherProfile(id: string, name: string): Promise<AetherProfileConfig> {
  return apiInvoke("aether/duplicate", { id, name });
}

export async function deleteAetherProfile(id: string): Promise<void> {
  await apiInvoke("aether/delete", { id });
}

export async function setAetherActiveProfile(id: string): Promise<AetherProfileConfig> {
  return apiInvoke("aether/set_active", { id });
}

export async function listProfiles(): Promise<ProfileSummary[]> {
  const res = await fetch(`${API_BASE}/list_profiles`);
  const text = await res.text();
  try {
    return JSON.parse(text) as ProfileSummary[];
  } catch {
    throw new Error(text);
  }
}

export async function switchProfile(id: string): Promise<void> {
  await apiInvoke("switch_profile", { id });
}

export async function createProfile(name: string): Promise<ProfileSummary> {
  return apiInvoke("create_profile", { name });
}

export async function duplicateProfile(id: string, name: string): Promise<ProfileSummary> {
  return apiInvoke("duplicate_profile", { id, name });
}

export async function renameProfile(id: string, name: string): Promise<ProfileSummary> {
  return apiInvoke("rename_profile", { id, name });
}

export async function deleteProfile(id: string): Promise<void> {
  await apiInvoke("delete_profile", { id });
}

export async function activeProfileId(): Promise<{ id: string }> {
  const res = await fetch(`${API_BASE}/active_profile_id`);
  const text = await res.text();
  try {
    return JSON.parse(text) as { id: string };
  } catch {
    throw new Error(text);
  }
}

export interface ChainNode {
  name: string;
  source: string;
  kind: string;
  delay: number | null;
  unusable: string | null;
}

export async function setChain(settings: ChainSettings): Promise<boolean> {
  return apiInvoke("set_chain", { settings });
}

export async function chainStatus(): Promise<ChainStatus> {
  return apiInvoke("chain_status");
}

export async function psiphonStatus(): Promise<PsiphonSnapshot> {
  return apiInvoke("psiphon_status");
}

export async function torStatus(): Promise<TorSnapshot> {
  return apiInvoke("tor_status");
}

export async function carriersAvailable(): Promise<CarrierKind[]> {
  return apiInvoke("carriers_available");
}

export async function fetchBridges(country: string): Promise<string[]> {
  return apiInvoke("fetch_bridges", { country });
}

export async function setPsiphonRegion(region: string): Promise<CoreSnapshot> {
  return apiInvoke("set_psiphon_region", { region });
}

export async function getPsiphonInfo(): Promise<PsiphonInfo> {
  return apiInvoke("psiphon_info");
}

export async function uploadPsiphonServerList(
  content: string | Uint8Array | Blob
): Promise<{ success: boolean; path: string; count: number }> {
  try {
    const res = await fetch(`${API_BASE}/upload_psiphon_server_list`, {
      method: "POST",
      headers: { "Content-Type": "application/octet-stream" },
      body: content as BodyInit,
    });
    const text = await res.text();
    let body: any;
    try {
      body = JSON.parse(text);
    } catch {
      body = { success: false, error: text };
    }
    if (!res.ok) throw new Error(body.error || `Upload failed (status ${res.status})`);
    return body;
  } catch (err: unknown) {
    if (err instanceof TypeError && err.message.toLowerCase().includes("failed to fetch")) {
      throw new Error("上传失败：网络请求被中断（Failed to fetch）。请确保后台服务正在运行并已允许大文件上传。");
    }
    throw err;
  }
}

export async function savePsiphonConfig(payload: {
  signaturePublicKey?: string;
  listenPort?: number;
  listenAddress?: string;
  egressRegion?: string;
  propagationChannelId?: string;
  sponsorId?: string;
}): Promise<{ success: boolean }> {
  return apiInvoke("save_psiphon_config", payload);
}

export async function startPsiphonStandalone(payload?: {
  egressRegion?: string;
  listenPort?: number;
  listenAddress?: string;
}): Promise<{
  success: boolean;
  address: string;
  snapshot: PsiphonSnapshot;
}> {
  return apiInvoke("start_psiphon", payload);
}

export async function stopPsiphonStandalone(): Promise<{
  success: boolean;
  snapshot: PsiphonSnapshot;
}> {
  return apiInvoke("stop_psiphon");
}

export interface LanStatus {
  running: boolean;
  address: string | null;
  open: boolean;
}

export async function setFullTunnel(enabled: boolean): Promise<boolean> {
  return apiInvoke("set_full_tunnel", { enabled });
}

export const NEEDS_ADMINISTRATOR = "needs-administrator";

export async function fullTunnelIsPermitted(): Promise<boolean> {
  return apiInvoke("full_tunnel_is_permitted");
}

export async function resumingFullTunnel(): Promise<boolean> {
  return apiInvoke("resuming_full_tunnel");
}

export async function restartAsAdministrator(): Promise<void> {
  return apiInvoke("restart_as_administrator");
}

export async function setLanShare(settings: LanSettings): Promise<LanStatus> {
  return apiInvoke("set_lan_share", { settings });
}

export async function lanShareStatus(): Promise<LanStatus> {
  return apiInvoke("lan_share_status");
}

export async function chainNodes(): Promise<ChainNode[]> {
  return apiInvoke("chain_nodes");
}

export async function chainTest(source: string, node: string): Promise<number | null> {
  return apiInvoke("chain_test", { source, node });
}

export async function chainSelect(node: string): Promise<void> {
  return apiInvoke("chain_select", { node });
}

export async function setSystemProxy(enabled: boolean): Promise<boolean> {
  return apiInvoke("set_system_proxy", { enabled });
}

export interface CoreItem {
  name: string;
  installed: boolean;
  version: string | null;
  path: string | null;
}

export async function getCoreInventory(): Promise<CoreItem[]> {
  const res = await fetch(`${API_BASE}/core_inventory`);
  if (!res.ok) throw new Error("Failed to get core inventory");
  return res.json();
}

export async function downloadCore(coreName: string): Promise<void> {
  const res = await fetch(`${API_BASE}/download_core`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ coreName }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({ error: "Download failed" }));
    throw new Error(data.error || "Failed to download core");
  }
  const data = await res.json();
  if (!data.success) throw new Error(data.error);
}

export async function uploadCore(coreName: string, file: File | Blob): Promise<{ success: boolean; path?: string }> {
  try {
    const res = await fetch(`${API_BASE}/upload_core/${encodeURIComponent(coreName)}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/octet-stream",
      },
      body: file,
    });
    if (!res.ok) {
      const text = await res.text();
      try {
        const json = JSON.parse(text);
        throw new Error(json.error || text);
      } catch (e: any) {
        if (e.message && e.message !== text) throw e;
        throw new Error(text || `Upload failed (status ${res.status})`);
      }
    }
    return res.json();
  } catch (err: unknown) {
    if (err instanceof TypeError && err.message.toLowerCase().includes("failed to fetch")) {
      throw new Error("上传失败：网络连接被中断（Failed to fetch）。请确保后台服务已重新运行最新版本并放行了大文件。");
    }
    throw err;
  }
}

// ── WarpScout ─────────────────────────────────────────────────────────────────

export interface WarpEndpoint {
  endpoint: string;
  country: string;
  node: string;
  nodeLocation: string;
  pingMs: number | null;
}

export interface WarpScoutStatus {
  installed: boolean;
  version: string | null;
  path: string | null;
  accountReady: boolean;
  /** "warpScout" | "aetherIdentity" | "none" */
  accountSource: "warpScout" | "aetherIdentity" | "none";
}

export interface WarpScanResult {
  endpoints: WarpEndpoint[];
  /** Raw stdout — shown when endpoints is empty for diagnostics */
  raw: string;
}

export interface WarpScanOptions {
  protocol?: "wg" | "awg" | "masque" | "masque-h2";
  country?: string;
  node?: string;
  excludeNode?: string;
  sample?: number;
  tunPing?: boolean;
}

export async function warpscoutStatus(): Promise<WarpScoutStatus> {
  return apiInvoke("warpscout_status");
}

export async function warpscoutInstall(): Promise<WarpScoutStatus> {
  return apiInvoke("warpscout_install");
}

export async function warpscoutBridgeAccount(): Promise<WarpScoutStatus> {
  return apiInvoke("warpscout_bridge");
}

/** Register a fresh WARP account via warpscout (fallback when no aether identity exists). */
export async function warpscoutRegister(): Promise<WarpScoutStatus> {
  return apiInvoke("warpscout_register");
}

export async function warpscoutScan(opts: WarpScanOptions): Promise<WarpScanResult> {
  return apiInvoke("warpscout_scan", opts);
}

// ── Routing rules (generic Aether direct & block lists) ───────────────────────

export async function getRoutingRulesInfo(): Promise<RoutingRulesInfo> {
  const res = await fetch(`${API_BASE}/routing_rules_info`);
  if (!res.ok) throw new Error(`Failed to fetch routing rules info: ${res.statusText}`);
  return res.json();
}

export async function uploadDirectRules(
  content: string | Uint8Array
): Promise<{ success: boolean; count: number; path: string }> {
  const body = typeof content === "string" ? content : (content as Uint8Array).buffer;
  const res = await fetch(`${API_BASE}/upload_direct_rules`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: body as BodyInit,
  });
  if (!res.ok) throw new Error(`Failed to upload direct rules: ${res.statusText}`);
  return res.json();
}

export async function uploadBlockRules(
  content: string | Uint8Array
): Promise<{ success: boolean; count: number; path: string }> {
  const body = typeof content === "string" ? content : (content as Uint8Array).buffer;
  const res = await fetch(`${API_BASE}/upload_block_rules`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: body as BodyInit,
  });
  if (!res.ok) throw new Error(`Failed to upload block rules: ${res.statusText}`);
  return res.json();
}

export async function uploadRoutesFile(
  content: string | Uint8Array
): Promise<{ success: boolean; blockCount: number; directCount: number }> {
  const body = typeof content === "string" ? content : (content as Uint8Array).buffer;
  const res = await fetch(`${API_BASE}/upload_routes_file`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: body as BodyInit,
  });
  if (!res.ok) throw new Error(`Failed to upload routes file: ${res.statusText}`);
  return res.json();
}

export async function clearRoutingList(
  ruleType: "direct" | "block" | "all"
): Promise<{ success: boolean }> {
  return apiInvoke("clear_routing_list", { ruleType });
}

// ── Proton / WireProxy API ──────────────────────────────────────────────────

export async function getProtonInfo(): Promise<ProtonInfo> {
  const res = await fetch(`${API_BASE}/proton_info`);
  if (!res.ok) throw new Error("Failed to fetch Proton info");
  return res.json();
}

export async function loginProtonGuest(): Promise<{ success: boolean; uid?: string; certExpiresAt?: number }> {
  return apiInvoke("proton_login_guest");
}

export async function renewProtonCert(): Promise<{ success: boolean; certExpiresAt?: number }> {
  return apiInvoke("proton_renew_cert");
}

export async function refreshProtonServers(): Promise<{ success: boolean; count: number }> {
  return apiInvoke("proton_refresh_servers");
}

export async function getProtonServers(country?: string): Promise<ProtonServerSummary[]> {
  const params = new URLSearchParams();
  if (country) params.append("country", country);
  const res = await fetch(`${API_BASE}/proton_servers?${params.toString()}`);
  if (!res.ok) throw new Error("Failed to fetch Proton servers");
  return res.json();
}

export async function saveProtonConfig(payload: {
  listenAddress?: string;
  listenPort?: number;
  country?: string;
  serverName?: string;
  autoFailover?: boolean;
}): Promise<{ success: boolean }> {
  return apiInvoke("save_proton_config", payload);
}

export async function startProton(payload?: {
  country?: string;
  serverName?: string;
  listenAddress?: string;
  listenPort?: number;
}): Promise<{ success: boolean; address?: string }> {
  return apiInvoke("start_proton", payload || {});
}

export async function stopProton(): Promise<{ success: boolean }> {
  return apiInvoke("stop_proton");
}

// ── Windscribe API ──────────────────────────────────────────────────────────

export async function getWindscribeStatus(): Promise<WindscribeStatusResponse> {
  const res = await fetch(`${API_BASE}/windscribe_status`);
  if (!res.ok) throw new Error("Failed to fetch Windscribe status");
  return res.json();
}

export async function loginWindscribe(payload: {
  username: string;
  password: string;
  upstreamProxy?: string;
}): Promise<{ success: boolean; account?: WindscribeAccount; error?: string }> {
  return apiInvoke("windscribe_login", payload);
}

export async function registerWindscribe(payload: {
  email?: string;
  upstreamProxy?: string;
}): Promise<{ success: boolean; account?: WindscribeAccount; error?: string }> {
  return apiInvoke("windscribe_register", payload);
}

export async function refreshWindscribe(payload?: {
  upstreamProxy?: string;
}): Promise<{ success: boolean; account?: WindscribeAccount; error?: string }> {
  return apiInvoke("windscribe_refresh", payload || {});
}

export async function saveWindscribeConfig(payload: {
  listenAddress?: string;
  listenPort?: number;
  country?: string;
  serverTag?: string;
  upstreamProxy?: string;
  autoFailover?: boolean;
}): Promise<{ success: boolean; error?: string }> {
  return apiInvoke("save_windscribe_config", payload);
}

export async function startWindscribe(payload?: {
  country?: string;
  serverTag?: string;
  listenAddress?: string;
  listenPort?: number;
}): Promise<{ success: boolean; address?: string; error?: string }> {
  return apiInvoke("start_windscribe", payload || {});
}

export async function stopWindscribe(): Promise<{ success: boolean; error?: string }> {
  return apiInvoke("stop_windscribe");
}

// ── Multi-instance SOCKS5 Management API ────────────────────────────────────

export async function listSocksInstances(): Promise<SocksInstanceView[]> {
  const res = await fetch(`${API_BASE}/socks_instances`);
  if (!res.ok) throw new Error("Failed to fetch SOCKS5 instances");
  return res.json();
}

export async function createSocksInstance(config: SocksInstanceConfig): Promise<SocksInstanceView> {
  return apiInvoke("socks_instances", config);
}

export async function updateSocksInstance(config: SocksInstanceConfig): Promise<SocksInstanceView> {
  return apiInvoke("socks_instances/update", config);
}

export async function deleteSocksInstance(id: string): Promise<{ success: boolean }> {
  return apiInvoke("socks_instances/delete", { id });
}

export async function startSocksInstance(id: string): Promise<SocksInstanceStatus> {
  return apiInvoke("socks_instances/start", { id });
}

export async function stopSocksInstance(id: string): Promise<SocksInstanceStatus> {
  return apiInvoke("socks_instances/stop", { id });
}

export async function restartSocksInstance(id: string): Promise<SocksInstanceStatus> {
  return apiInvoke("socks_instances/restart", { id });
}

export async function testSocksConnectivity(id: string): Promise<ConnectivityResult> {
  return apiInvoke("socks_instances/test_connectivity", { id });
}

export async function testSocksSpeed(id: string): Promise<SocksSpeedResult> {
  return apiInvoke("socks_instances/test_speed", { id });
}

export async function setSocksAutostart(id: string, autostart: boolean): Promise<{ success: boolean; autostart: boolean }> {
  return apiInvoke("socks_instances/set_autostart", { id, autostart });
}


