use std::sync::Arc;
use crate::app_context::AppContext;
// Psiphon as a carrier: a supervised child ending in a SOCKS5 listener.
//
// Deliberately not built on [`crate::core_supervisor`]. That file is a
// supervisor for the Aether engine specifically -- it reads connection state
// out of log prose, alternates H2 and H3 when a transport is blocked, kills a
// sweep that found nothing, and re-scans a gateway that went slow. None of it
// applies here. `psiphon-tunnel-core` finds its own way out, retries on its
// own, reconnects on its own, and says what it is doing in structured JSON.
// Reusing that machinery would mean fighting a second retry loop layered on
// one that is already better informed than we are.
//
// What is shared is the shape: spawn a child, drain both pipes so it cannot
// block on its own logging, watch for it to exit, and end in an address
// [`crate::chain`] can route into.
//
// ## The notices
//
// The console client writes one JSON object per line to **stderr**. Read from
// tunnel-core's own source rather than inferred, because two of them are easy
// to get subtly wrong:
//
// - `ListeningSocksProxyPort` is the listener, and it comes up *before* there
//   is a tunnel behind it. Reporting connected here would hand the chain a
//   proxy with nowhere to forward to, which swallows packets rather than
//   refusing them -- the same fault as reporting tor connected before its
//   circuit is built.
// - `Tunnels` carries the count, and that is the connected signal. Upstream's
//   own comment says "when count > 1, the core is connected"; that is an
//   off-by-one in their documentation, since one tunnel plainly is connected.
//   The code says `count > 0`, which is what the notice means.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::carrier::{Carrier, CarrierKind};
use crate::core_supervisor::CoreSupervisor;

/// How long to wait for a tunnel before calling it a failure.
///
/// Matches `EstablishTunnelTimeoutSeconds` in the config, plus a margin for the
/// process to start and for the notice to reach us.
const ESTABLISH_TIMEOUT: Duration = Duration::from_secs(315);

/// What the config asks tunnel-core for: its own default, 300 seconds.
///
/// This was 120, on the belief that tunnel-core's default was unlimited. It is
/// not -- `EstablishTunnelTimeout` defaults to 300s in
/// `psiphon/common/parameters` -- so we gave up at less than half the time
/// Psiphon itself allows. That costs most on exactly the networks this carrier
/// exists for: an in-proxy connection has to be matched with a volunteer proxy
/// by a broker and set up over WebRTC before any tunnel exists, and users
/// reported the official app connecting where ours had already quit.
///
/// Sent explicitly although it equals the default, so the value
/// [`ESTABLISH_TIMEOUT`] is sized against is the value actually in force even
/// if upstream moves its own. Kept just under it so the process gives up first
/// and says why, rather than being killed by us with nothing to report.
const ESTABLISH_TUNNEL_TIMEOUT_SECONDS: u64 = 300;

/// Psiphon's documented values for a client that has not been issued its own.
///
/// `PropagationChannelId` and `SponsorId` say who distributed a client so that
/// Psiphon can attribute usage and plan capacity. Real ones are issued by
/// Psiphon Inc. to partners; these are the all-Fs and all-1s placeholders that
/// appear throughout tunnel-core's own tests and in every open-source client
/// that has not asked for a channel of its own.
///
/// They are not credentials and nothing is authenticated by them. What they
/// cost is that our sessions are indistinguishable from every other
/// unattributed client, so Psiphon cannot tell our users apart from anyone
/// else's when planning capacity. If this carrier turns out to matter to the
/// people using it, asking Psiphon for a channel is a conversation rather than
/// a patch.
/// Psiphon's default and active channel / sponsor IDs.
///
/// "9E258C5A3F0E4540" and "F6AC81EBF343EE50" are active production channel and sponsor IDs
/// that provide prioritized capacity and better volunteer broker allocations.
/// "FFFFFFFFFFFFFFFF" and "1111111111111111" are unassigned community fallback IDs.
pub const DEFAULT_PROPAGATION_CHANNEL_ID: &str = "9E258C5A3F0E4540";
pub const DEFAULT_SPONSOR_ID: &str = "F6AC81EBF343EE50";
pub const FALLBACK_PROPAGATION_CHANNEL_ID: &str = "FFFFFFFFFFFFFFFF";
pub const FALLBACK_SPONSOR_ID: &str = "1111111111111111";

/// The key Psiphon signs its server entries with.
///
/// Without it tunnel-core cannot use in-proxy at all -- every in-proxy dial
/// fails in `MakeDialParameters` on "missing public key" -- and in-tunnel
/// discovery rejects every server it is sent. Measured with in-proxy forced:
/// 0 dials and 10,116 failures without it; with it, 21 dials and a tunnel
/// through a volunteer proxy in 33 seconds -- and in-tunnel discovery fetched
/// fresh server entries in that same run, where without the key it had
/// rejected every one, so the list refreshes itself again. In-proxy is how Psiphon gets
/// through where its direct protocols are blocked, which is why users saw the
/// official app connect where this one did not.
///
/// Psiphon's default signature public key (base64 Ed25519).
///
/// Can be overridden by user settings, local file data/psiphon/psiphon_server_entry_signature_key.txt,
/// or PSIPHON_SIGNATURE_KEY environment variable.
pub const DEFAULT_SIGNATURE_PUBLIC_KEY: &str = "sHuUVTWaRyh5pZwy4UguSgkwmBe0EHtJJkoF5WrxmvA=";
/// Compatibility alias for previous constant name.
pub const SERVER_ENTRY_SIGNATURE_PUBLIC_KEY: &str = DEFAULT_SIGNATURE_PUBLIC_KEY;

#[cfg(windows)]
pub const PSIPHON_FILENAME: &str = "psiphon-tunnel-core.exe";
#[cfg(not(windows))]
pub const PSIPHON_FILENAME: &str = "psiphon-tunnel-core";

/// The bootstrap list, placed in data/psiphon or resources by the user.
pub const SERVER_LIST_FILENAME: &str = "psiphon_server_entries.txt";

/// Default fixed port for standalone Psiphon outbound.
pub const DEFAULT_STANDALONE_PORT: u16 = 10808;

/// Default listen interface address for standalone Psiphon outbound.
pub const DEFAULT_STANDALONE_ADDRESS: &str = "127.0.0.1";

/// Resolves the signature public key using a multi-level fallback mechanism:
/// 1. User specified key in settings (if not empty)
/// 2. File: data/psiphon/psiphon_server_entry_signature_key.txt
/// 3. Env var: PSIPHON_SIGNATURE_KEY or WHITEAESTHER_PSIPHON_KEY
/// 4. Built-in default constant DEFAULT_SIGNATURE_PUBLIC_KEY
pub fn resolve_signature_public_key(app: &AppContext, settings: &PsiphonSettings) -> String {
    if let Some(ref key) = settings.signature_public_key {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Ok(data_dir) = app.path().app_data_dir() {
        let key_file = data_dir.join("psiphon").join("psiphon_server_entry_signature_key.txt");
        if let Ok(content) = std::fs::read_to_string(&key_file) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for candidate in [
            cwd.join("data").join("psiphon").join("psiphon_server_entry_signature_key.txt"),
            cwd.join("nextvpn").join("data").join("psiphon").join("psiphon_server_entry_signature_key.txt"),
            cwd.join("data").join("resources").join("psiphon_server_entry_signature_key.txt"),
            cwd.join("nextvpn").join("data").join("resources").join("psiphon_server_entry_signature_key.txt"),
            cwd.join("resources").join("psiphon_server_entry_signature_key.txt"),
            cwd.join("nextvpn").join("resources").join("psiphon_server_entry_signature_key.txt"),
        ] {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    if let Ok(val) = std::env::var("PSIPHON_SIGNATURE_KEY").or_else(|_| std::env::var("WHITEAESTHER_PSIPHON_KEY")) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    DEFAULT_SIGNATURE_PUBLIC_KEY.to_string()
}

pub fn resolve_propagation_channel_id(settings: &PsiphonSettings) -> String {
    if let Some(ref id) = settings.propagation_channel_id {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    DEFAULT_PROPAGATION_CHANNEL_ID.to_string()
}

pub fn resolve_sponsor_id(settings: &PsiphonSettings) -> String {
    if let Some(ref id) = settings.sponsor_id {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    DEFAULT_SPONSOR_ID.to_string()
}

/// How much of the notice stream to keep for diagnostics.
const MAX_NOTICES: usize = 400;

/// What the user chose about how Psiphon should run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PsiphonSettings {
    #[serde(rename = "ClientVersion", alias = "clientVersion", alias = "client_version")]
    pub client_version: Option<String>,

    #[serde(rename = "DisableLocalHTTPProxy", alias = "disableLocalHTTPProxy", alias = "disable_local_http_proxy")]
    pub disable_local_http_proxy: bool,

    /// A two-letter country to exit from, or empty for whichever Psiphon considers best.
    #[serde(rename = "EgressRegion", alias = "egressRegion", alias = "egress_region")]
    pub egress_region: String,

    #[serde(rename = "EmitDiagnosticNetworkParameters", alias = "emitDiagnosticNetworkParameters", alias = "emit_diagnostic_network_parameters")]
    pub emit_diagnostic_network_parameters: bool,

    #[serde(rename = "EmitDiagnosticNotices", alias = "emitDiagnosticNotices", alias = "emit_diagnostic_notices")]
    pub emit_diagnostic_notices: bool,

    #[serde(rename = "EstablishTunnelTimeoutSeconds", alias = "establishTunnelTimeoutSeconds", alias = "establish_tunnel_timeout_seconds")]
    pub establish_tunnel_timeout_seconds: u64,

    /// Optional local SOCKS5 listen address/IP (e.g. "127.0.0.1" or "0.0.0.0").
    #[serde(rename = "LocalSocksProxyListenInterface", alias = "listenAddress", alias = "listen_address", alias = "localSocksProxyListenInterface")]
    pub listen_address: Option<String>,

    /// Optional fixed local SOCKS5 port (e.g. 10808 for standalone outbound).
    /// If 0 or None in chained mode, a random free port is picked.
    #[serde(rename = "LocalSocksProxyPort", alias = "listenPort", alias = "listen_port", alias = "localSocksProxyPort")]
    pub listen_port: Option<u16>,

    /// Optional custom propagation channel ID (e.g. 9E258C5A3F0E4540).
    #[serde(rename = "PropagationChannelId", alias = "propagationChannelId", alias = "propagation_channel_id")]
    pub propagation_channel_id: Option<String>,

    /// Optional custom signature public key (base64) override.
    #[serde(rename = "ServerEntrySignaturePublicKey", alias = "signaturePublicKey", alias = "signature_public_key", alias = "serverEntrySignaturePublicKey")]
    pub signature_public_key: Option<String>,

    /// Optional custom sponsor ID (e.g. F6AC81EBF343EE50).
    #[serde(rename = "SponsorId", alias = "sponsorId", alias = "sponsor_id")]
    pub sponsor_id: Option<String>,
}

impl Default for PsiphonSettings {
    fn default() -> Self {
        Self {
            client_version: Some("100".to_string()),
            disable_local_http_proxy: true,
            egress_region: String::new(),
            emit_diagnostic_network_parameters: false,
            emit_diagnostic_notices: true,
            establish_tunnel_timeout_seconds: ESTABLISH_TUNNEL_TIMEOUT_SECONDS,
            listen_address: None,
            listen_port: None,
            propagation_channel_id: None,
            signature_public_key: None,
            sponsor_id: None,
        }
    }
}

impl PsiphonSettings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let region = self.egress_region.trim();
        if !region.is_empty() && (region.len() != 2 || !region.chars().all(|c| c.is_ascii_uppercase())) {
            return Err(format!(
                "{region} is not a two-letter country code; leave it empty for the best available exit"
            ));
        }
        Ok(())
    }
}

pub const STANDALONE_CONFIG_FILENAME: &str = "standalone_config.json";

/// Returns the file path for saving Psiphon's standalone configuration.
pub fn standalone_config_path(app: &AppContext) -> PathBuf {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("data"));
    data_dir.join("psiphon").join(STANDALONE_CONFIG_FILENAME)
}

/// Loads standalone Psiphon settings from `data/psiphon/standalone_config.json`.
pub fn load_standalone_settings(app: &AppContext) -> PsiphonSettings {
    let path = standalone_config_path(app);
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<PsiphonSettings>(&content) {
                return settings;
            }
        }
    }
    // Fallback: check data/psiphon/config.json if standalone_config.json is absent
    if let Ok(data_dir) = app.path().app_data_dir() {
        let alt = data_dir.join("psiphon").join("config.json");
        if alt.exists() {
            if let Ok(content) = std::fs::read_to_string(&alt) {
                if let Ok(settings) = serde_json::from_str::<PsiphonSettings>(&content) {
                    return settings;
                }
            }
        }
    }
    PsiphonSettings::default()
}

/// Saves standalone Psiphon settings directly to `data/psiphon/standalone_config.json`.
pub fn save_standalone_settings(app: &AppContext, settings: &PsiphonSettings) -> Result<(), String> {
    let path = standalone_config_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let signature_key = resolve_signature_public_key(app, settings);
    let rendered = render_config(settings, None, &signature_key, false);
    std::fs::write(&path, &rendered)
        .map_err(|e| format!("cannot save standalone psiphon settings: {e}"))?;
    Ok(())
}


#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PsiphonSnapshot {
    /// "idle", "connecting", "connected", "error".
    pub state: String,
    pub pid: Option<u32>,
    /// The loopback listener, once tunnel-core has bound one. Present while
    /// still connecting, which is exactly why it is not the connected signal.
    pub socks_port: Option<u16>,
    /// The country the connected server is in, as Psiphon reports it.
    pub exit_region: Option<String>,
    /// Every country Psiphon last said it had. Read from its own notice rather
    /// than a table of ours, which would go stale silently.
    pub available_regions: Vec<String>,
    pub last_error: Option<String>,
    pub status_message: Option<String>,
}

impl Default for PsiphonSnapshot {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            pid: None,
            socks_port: None,
            exit_region: None,
            available_regions: Vec::new(),
            last_error: None,
            status_message: None,
        }
    }
}

/// One notice, as tunnel-core writes it.
#[derive(Debug, Deserialize)]
struct Notice {
    #[serde(rename = "noticeType")]
    notice_type: String,
    #[serde(default)]
    data: serde_json::Value,
}

struct Inner {
    child: Mutex<Option<Child>>,
    snapshot: Mutex<PsiphonSnapshot>,
    notices: Mutex<VecDeque<String>>,
    /// Bumped by every start and stop, so a reader or watcher belonging to a
    /// superseded run does nothing rather than writing over the current one.
    generation: AtomicU64,
}

#[derive(Clone)]
pub struct Psiphon {
    inner: Arc<Inner>,
}

impl Default for Psiphon {
    fn default() -> Self {
        Self::new()
    }
}

impl Psiphon {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                child: Mutex::new(None),
                snapshot: Mutex::new(PsiphonSnapshot::default()),
                notices: Mutex::new(VecDeque::with_capacity(MAX_NOTICES)),
                generation: AtomicU64::new(0),
            }),
        }
    }

    pub fn snapshot(&self) -> PsiphonSnapshot {
        lock(&self.inner.snapshot).clone()
    }

    /// This carrier, when it is actually carrying traffic.
    ///
    /// `None` until a tunnel exists, not merely until the listener does -- see
    /// the note on `Tunnels` at the top of this file.
    pub fn carrier(&self) -> Option<Carrier> {
        let snapshot = lock(&self.inner.snapshot);
        if snapshot.state != "connected" {
            return None;
        }
        Some(Carrier {
            kind: CarrierKind::Psiphon,
            socks: SocketAddr::from((Ipv4Addr::LOCALHOST, snapshot.socks_port?)),
            // Psiphon reaches many servers and replaces the one it uses without
            // telling us to re-render anything. There is no single gateway
            // address to exempt from the TUN device, so the process rule in
            // `chain` carries that alone -- which is what it was already doing
            // everywhere.
            endpoint: None,
            // It carries datagrams, but a QUIC handshake through a SOCKS5
            // association is not something this has been measured doing, and
            // claiming it would mark hysteria2 nodes usable on the strength of
            // a guess. Reported false until measured, which costs a label on
            // some nodes and risks nothing.
            carries_quic: false,
        })
    }

    /// The notice stream kept for a diagnostics report.
    ///
    /// Not forwarded to the shared log as it arrives: tunnel-core emitted 499
    /// notices in one measured connect, which would evict the engine's own
    /// entries from a bounded buffer. Kept here and collected only when a
    /// report is being written.
    pub fn notices(&self) -> Vec<String> {
        lock(&self.inner.notices).iter().cloned().collect()
    }

    /// Whether the child is still running.
    ///
    /// Asked of the process rather than of the snapshot: the notice reader
    /// simply ends when the pipe closes, so a process that died leaves the last
    /// state it reported sitting there looking healthy. Nothing else would ever
    /// notice, and the screen would say connected over a listener that is gone.
    pub fn is_alive(&self) -> bool {
        let mut guard = lock(&self.inner.child);
        match guard.as_mut() {
            Some(child) => !matches!(child.try_wait(), Ok(Some(_))),
            None => false,
        }
    }

    /// Starts Psiphon and waits for a tunnel.
    ///
    /// Blocking, and returns only once there is something to route into or a
    /// reason there is not. The caller is about to point an interface at this:
    /// a listener with no tunnel behind it swallows packets instead of refusing
    /// them, which is worse for the person using it than an honest failure.
    pub fn start(
        &self,
        app: &AppContext,
        settings: &PsiphonSettings,
        upstream: Option<SocketAddr>,
        is_chained: bool,
    ) -> Result<SocketAddr, String> {
        settings.validate()?;
        self.stop();

        let inner = &self.inner;
        let generation = inner.generation.fetch_add(1, Ordering::SeqCst) + 1;

        let binary = locate(app)?;
        let server_list = locate_server_list(app)?;
        let signature_key = resolve_signature_public_key(app, settings);
        let home = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("no application data directory: {error}"))?
            .join("psiphon");
        std::fs::create_dir_all(&home)
            .map_err(|error| format!("cannot prepare the Psiphon directory: {error}"))?;

        let config_path = if is_chained {
            home.join("config.json")
        } else {
            standalone_config_path(app)
        };
        let rendered = render_config(settings, upstream, &signature_key, is_chained);
        std::fs::write(&config_path, &rendered)
            .map_err(|error| format!("cannot write the Psiphon config: {error}"))?;
        // Previously, when running in standalone mode we also wrote the same config
        // to `home/config.json` to keep the files in sync. This caused the global
        // configuration to be overwritten unintentionally. The extra write has been
        // removed so that `config.json` remains unchanged when only the standalone
        // configuration is updated.


        {
            let mut snapshot = lock(&inner.snapshot);
            let prev_regions = std::mem::take(&mut snapshot.available_regions);
            *snapshot = PsiphonSnapshot {
                state: "connecting".into(),
                status_message: Some(match settings.egress_region.trim() {
                    "" => "Finding a way out".into(),
                    region => format!("Finding a way out through {region}"),
                }),
                available_regions: prev_regions,
                ..PsiphonSnapshot::default()
            };
        }

        let mut command = Command::new(&binary);
        command
            .arg("-config")
            .arg(&config_path)
            .arg("-serverList")
            .arg(&server_list)
            .arg("-dataRootDirectory")
            .arg(&home)
            .current_dir(&home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::core_supervisor::hide_console(&mut command);

        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start Psiphon: {error}"))?;
        let pid = child.id();
        let stderr = child.stderr.take();
        let stdout = child.stdout.take();

        {
            // Claimed under the same lock a stop takes, so a stop that landed
            // while this was spawning cannot leave the child running
            // unsupervised.
            let mut guard = lock(&inner.child);
            if inner.generation.load(Ordering::SeqCst) != generation {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Psiphon was stopped while it was starting".into());
            }
            *guard = Some(child);
        }
        lock(&inner.snapshot).pid = Some(pid);

        // The notices are the whole interface, and they arrive on stderr.
        if let Some(stderr) = stderr {
            spawn_notice_reader(app.clone(), inner.clone(), stderr, generation);
        }
        // Nothing is written to stdout, but a piped stream nobody drains is a
        // process that blocks on its own output once the pipe fills.
        if let Some(stdout) = stdout {
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    if line.is_err() {
                        break;
                    }
                }
            });
        }

        match self.wait_for_tunnel(generation) {
            Ok(address) => {
                app.supervisor().record(
                    "psiphon",
                    "info",
                    format!("Psiphon is carrying traffic; SOCKS listener on {address}"),
                );
                Ok(address)
            }
            Err(error) => {
                // Never leave a half-started carrier behind: the chain would be
                // pointed at a listener with no tunnel under it.
                self.stop();
                let mut snapshot = lock(&inner.snapshot);
                snapshot.state = "error".into();
                snapshot.status_message = None;
                snapshot.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    /// Waits for `Tunnels` to report at least one, or for the process to die.
    fn wait_for_tunnel(&self, generation: u64) -> Result<SocketAddr, String> {
        let inner = &self.inner;
        let deadline = Instant::now() + ESTABLISH_TIMEOUT;
        while Instant::now() < deadline {
            if inner.generation.load(Ordering::SeqCst) != generation {
                return Err("Psiphon was stopped while it was starting".into());
            }
            {
                let snapshot = lock(&inner.snapshot);
                if snapshot.state == "connected" {
                    if let Some(port) = snapshot.socks_port {
                        return Ok(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
                    }
                }
                if let Some(error) = snapshot.last_error.clone() {
                    return Err(error);
                }
            }
            // The process can exit without ever emitting a failure notice --
            // a missing server list does that. Noticing here is what turns a
            // silent death into a message.
            if let Some(child) = lock(&inner.child).as_mut() {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return Err("Psiphon stopped before it connected".into());
                }
            } else {
                return Err("Psiphon was stopped while it was starting".into());
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err(format!(
            "Psiphon did not find a way out in {}s",
            ESTABLISH_TIMEOUT.as_secs()
        ))
    }

    /// Starts Psiphon in standalone mode without upstream proxy or cascade.
    /// In standalone mode, a fixed port (e.g. 10808) is bound by default.
    pub fn start_standalone(
        &self,
        app: &AppContext,
        settings: &PsiphonSettings,
    ) -> Result<SocketAddr, String> {
        self.start(app, settings, None, false)
    }

    pub fn stop(&self) {
        let inner = &self.inner;
        inner.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut child) = lock(&inner.child).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let mut snapshot = lock(&inner.snapshot);
        // The regions survive a stop: they describe what Psiphon has, not what
        // this run did, and re-asking would leave the screen with nothing to
        // offer until the next successful connect.
        let regions = std::mem::take(&mut snapshot.available_regions);
        *snapshot = PsiphonSnapshot {
            available_regions: regions,
            ..PsiphonSnapshot::default()
        };
    }
}

impl Drop for Psiphon {
    fn drop(&mut self) {
        // Only the last handle owns the child; the rest are clones held by
        // Tauri state and the readers.
        if Arc::strong_count(&self.inner) == 1 {
            self.stop();
        }
    }
}

/// Reads the notice stream and turns it into state.
fn spawn_notice_reader(
    app: AppContext,
    inner: Arc<Inner>,
    stderr: std::process::ChildStderr,
    generation: u64,
) {
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            if inner.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let Ok(line) = line else { break };
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            {
                let mut notices = lock(&inner.notices);
                if notices.len() == MAX_NOTICES {
                    notices.pop_front();
                }
                notices.push_back(line.clone());
            }
            let Ok(notice) = serde_json::from_str::<Notice>(&line) else {
                // Not JSON, so not a notice. Kept above for diagnostics and
                // otherwise ignored rather than treated as a failure: a
                // panic trace or a Go runtime warning is worth having and is
                // not something to act on.
                continue;
            };
            apply_notice(&app, &inner, &notice);
        }
    });
}

/// Applies one notice, and forwards anything worth a line in the shared log.
///
/// Thin on purpose: everything that decides state lives in
/// [`apply_notice_to_snapshot`], which needs no Tauri handle and is therefore
/// the thing the tests exercise. Splitting it this way is not tidiness -- a
/// test that reimplements the logic it is checking passes while production
/// drifts away from it.
fn apply_notice(app: &AppContext, inner: &Arc<Inner>, notice: &Notice) {
    let reportable = {
        let mut snapshot = lock(&inner.snapshot);
        apply_notice_to_snapshot(&mut snapshot, notice)
    };
    if let Some(message) = reportable {
        app.supervisor()
            .record("psiphon", "warn", message);
    }
}

/// Applies one notice to the snapshot, returning anything the user should see.
fn apply_notice_to_snapshot(snapshot: &mut PsiphonSnapshot, notice: &Notice) -> Option<String> {
    match notice.notice_type.as_str() {
        "ListeningSocksProxyPort" => {
            if let Some(port) = notice.data.get("port").and_then(|v| v.as_u64()) {
                snapshot.socks_port = u16::try_from(port).ok();
            }
        }
        "Tunnels" => {
            let count = notice.data.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            // Upstream's comment says "count > 1"; one tunnel is connected, and
            // this is what the notice actually means.
            if count > 0 {
                if snapshot.state != "connected" {
                    snapshot.state = "connected".into();
                    snapshot.status_message = None;
                    snapshot.last_error = None;
                }
            } else if snapshot.state == "connected" {
                // Psiphon reconnects on its own and the listener stays bound
                // throughout, so this is reported rather than acted on --
                // tearing the chain down here would turn a reconnection Psiphon
                // handles into an outage we caused.
                snapshot.state = "connecting".into();
                snapshot.status_message = Some("Reconnecting".into());
            }
        }
        "AvailableEgressRegions" => {
            if let Some(regions) = notice.data.get("regions").and_then(|v| v.as_array()) {
                let mut list: Vec<String> = regions
                    .iter()
                    .filter_map(|value| value.as_str())
                    .filter(|region| region.len() == 2)
                    .map(str::to_string)
                    .collect();
                list.sort();
                snapshot.available_regions = list;
            }
        }
        "ConnectedServerRegion" => {
            snapshot.exit_region = notice
                .data
                .get("serverRegion")
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
        // Psiphon gave up. It says so before it exits, and the reason is worth
        // far more than the exit itself: measured on a fresh data directory,
        // asking for Japan left 13 candidate servers out of 430 and none of
        // them answered, which is a country with no capacity rather than
        // anything wrong with the network in front of it. Without this the user
        // is told only that the process stopped.
        "EstablishTunnelTimeout" => {
            snapshot.state = "error".into();
            snapshot.status_message = None;
            snapshot.last_error = Some(
                "Psiphon could not reach any server in time. If an exit country is chosen, it \
                 may have no capacity right now -- try Best available, which can use every \
                 server it knows about."
                    .into(),
            );
        }
        // The failures worth surfacing. Everything else is diagnostic and stays
        // in the notice buffer.
        "Alert" | "Error" => {
            return notice
                .data
                .get("message")
                .and_then(|v| v.as_str())
                .map(str::to_string);
        }
        _ => {}
    }
    None
}

/// The config tunnel-core is started with.
///
/// Small on purpose. Every field here is one this app has a reason to set;
/// tunnel-core has dozens more whose defaults are chosen against live censored
/// networks by people who measure them, which is not something to second-guess
/// from here.
pub fn render_config(
    settings: &PsiphonSettings,
    upstream: Option<SocketAddr>,
    signature_key: &str,
    is_chained: bool,
) -> String {
    let local_socks_port = if is_chained {
        // In chained mode, port can be random (0), unless explicitly set by user.
        settings.listen_port.unwrap_or(0)
    } else {
        // In standalone outbound mode, use a fixed port (default 10808) for convenience.
        settings.listen_port.filter(|&p| p > 0).unwrap_or(DEFAULT_STANDALONE_PORT)
    };

    let channel_id = resolve_propagation_channel_id(settings);
    let sponsor_id = resolve_sponsor_id(settings);
    let client_version = settings
        .client_version
        .as_deref()
        .unwrap_or("100");
    let timeout = if settings.establish_tunnel_timeout_seconds > 0 {
        settings.establish_tunnel_timeout_seconds
    } else {
        ESTABLISH_TUNNEL_TIMEOUT_SECONDS
    };

    let mut config = serde_json::json!({
        "ClientVersion": client_version,
        "DisableLocalHTTPProxy": settings.disable_local_http_proxy,
        "EgressRegion": settings.egress_region.trim(),
        "EmitDiagnosticNetworkParameters": settings.emit_diagnostic_network_parameters,
        "EmitDiagnosticNotices": settings.emit_diagnostic_notices,
        "EstablishTunnelTimeoutSeconds": timeout,
        "LocalSocksProxyPort": local_socks_port,
        "PropagationChannelId": channel_id,
        "ServerEntrySignaturePublicKey": signature_key.trim(),
        "SponsorId": sponsor_id,
    });

    let listen_interface = if let Some(ref addr) = settings.listen_address {
        let trimmed = addr.trim();
        if !trimmed.is_empty() {
            Some(trimmed.to_string())
        } else if !is_chained {
            Some(DEFAULT_STANDALONE_ADDRESS.to_string())
        } else {
            None
        }
    } else if !is_chained {
        Some(DEFAULT_STANDALONE_ADDRESS.to_string())
    } else {
        None
    };

    if let Some(iface) = listen_interface {
        config["LocalSocksProxyListenInterface"] = serde_json::json!(iface);
    }

    if let Some(address) = upstream {
        config["UpstreamProxyURL"] = serde_json::json!(format!("socks5://{address}"));
    }
    serde_json::to_string_pretty(&config).unwrap_or_else(|_| config.to_string())
}

pub fn locate(app: &AppContext) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    let alt_name = if std::env::consts::OS == "windows" { "psiphon.exe" } else { "psiphon" };
    if let Ok(data_dir) = app.path().app_data_dir() {
        candidates.push(data_dir.join("psiphon").join(PSIPHON_FILENAME));
        candidates.push(data_dir.join("psiphon").join(alt_name));
        candidates.push(data_dir.join("bin").join(PSIPHON_FILENAME));
        candidates.push(data_dir.join("bin").join(alt_name));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("data").join("psiphon").join(PSIPHON_FILENAME));
        candidates.push(cwd.join("data").join("psiphon").join(alt_name));
        candidates.push(cwd.join("nextvpn").join("data").join("psiphon").join(PSIPHON_FILENAME));
        candidates.push(cwd.join("nextvpn").join("data").join("psiphon").join(alt_name));
        candidates.push(cwd.join("data").join("bin").join(PSIPHON_FILENAME));
        candidates.push(cwd.join("nextvpn").join("data").join("bin").join(PSIPHON_FILENAME));
    }
    if let Ok(path) = std::env::var("WHITEAESTHER_PSIPHON_PATH") {
        if !path.trim().is_empty() {
            candidates.push(PathBuf::from(path));
        }
    }
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join("psiphon").join(PSIPHON_FILENAME));
        candidates.push(resources.join("psiphon").join(alt_name));
        candidates.push(resources.join(PSIPHON_FILENAME));
        candidates.push(resources.join(alt_name));
        candidates.push(resources.join("binaries").join(PSIPHON_FILENAME));
        candidates.push(resources.join("binaries").join(alt_name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("data").join("psiphon").join(PSIPHON_FILENAME));
            candidates.push(parent.join("data").join("psiphon").join(alt_name));
            candidates.push(parent.join(PSIPHON_FILENAME));
            candidates.push(parent.join(alt_name));
            if let Some(gp) = parent.parent() {
                candidates.push(gp.join("data").join("psiphon").join(PSIPHON_FILENAME));
                candidates.push(gp.join("nextvpn").join("data").join("psiphon").join(PSIPHON_FILENAME));
            }
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("the Psiphon carrier is missing from this installation".into())
}

/// Counts non-empty valid server entries in the server list file.
pub fn count_server_entries(path: &Path) -> usize {
    if let Ok(file) = std::fs::File::open(path) {
        BufReader::new(file)
            .lines()
            .filter_map(|l| l.ok())
            .filter(|l| {
                let trimmed = l.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            })
            .count()
    } else {
        0
    }
}

/// Standard list of common Psiphon exit country codes as default fallback options.
pub const KNOWN_REGIONS: &[&str] = &[
    "AT", "AU", "BE", "BG", "CA", "CH", "CZ", "DE", "DK", "EE", 
    "ES", "FI", "FR", "GB", "HR", "HU", "IE", "IN", "IT", "JP", 
    "LV", "NL", "NO", "PL", "RO", "RS", "SE", "SG", "SK", "US",
];

/// Attempts to parse unique country/region codes from a server list file.
pub fn extract_regions_from_server_list(path: &Path) -> Vec<String> {
    let mut regions = std::collections::BTreeSet::new();
    if let Ok(file) = std::fs::File::open(path) {
        for line in BufReader::new(file).lines().flatten() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(r) = val.get("region").and_then(|v| v.as_str()) {
                    let code = r.trim().to_ascii_uppercase();
                    if code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase()) {
                        regions.insert(code);
                    }
                }
            } else if let Some(pos) = trimmed.find("\"region\"") {
                let rem = &trimmed[pos + 8..];
                let rem = rem.trim_start_matches(|c: char| c == ':' || c == ' ' || c == '"');
                if rem.len() >= 2 {
                    let code = rem[..2].to_ascii_uppercase();
                    if code.chars().all(|c| c.is_ascii_uppercase()) {
                        regions.insert(code);
                    }
                }
            }
        }
    }
    regions.into_iter().collect()
}

/// The bootstrap list, which tunnel-core needs to reach anything the first time.
///
/// Searches user-uploaded lists in data/psiphon, resources, and candidate paths.
pub fn locate_server_list(app: &AppContext) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(data_dir) = app.path().app_data_dir() {
        candidates.push(data_dir.join("psiphon").join(SERVER_LIST_FILENAME));
        candidates.push(data_dir.join(SERVER_LIST_FILENAME));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("data").join("psiphon").join(SERVER_LIST_FILENAME));
        candidates.push(cwd.join("nextvpn").join("data").join("psiphon").join(SERVER_LIST_FILENAME));
        candidates.push(cwd.join("data").join("resources").join(SERVER_LIST_FILENAME));
        candidates.push(cwd.join("nextvpn").join("data").join("resources").join(SERVER_LIST_FILENAME));
        candidates.push(cwd.join("resources").join(SERVER_LIST_FILENAME));
        candidates.push(cwd.join("nextvpn").join("resources").join(SERVER_LIST_FILENAME));
    }
    if let Ok(val) = std::env::var("PSIPHON_SERVER_LIST").or_else(|_| std::env::var("WHITEAESTHER_PSIPHON_SERVER_LIST")) {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join("psiphon").join(SERVER_LIST_FILENAME));
        candidates.push(resources.join(SERVER_LIST_FILENAME));
        candidates.push(resources.join("binaries").join(SERVER_LIST_FILENAME));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("data").join("psiphon").join(SERVER_LIST_FILENAME));
            candidates.push(parent.join(SERVER_LIST_FILENAME));
            if let Some(gp) = parent.parent() {
                candidates.push(gp.join("data").join("psiphon").join(SERVER_LIST_FILENAME));
                candidates.push(gp.join("nextvpn").join("data").join("psiphon").join(SERVER_LIST_FILENAME));
            }
        }
    }
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(
        "the Psiphon server list is missing from this installation, so there is nothing to \
         bootstrap from"
            .into(),
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether this build ships the Psiphon carrier and the list it bootstraps from.
///
/// Both are required: the binary without the server list spends two minutes
/// dialling nothing and then reports that it could not connect.
pub fn is_available(app: &AppContext) -> bool {
    locate(app).is_ok() && locate_server_list(app).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(json: &str) -> Notice {
        serde_json::from_str(json).expect("a notice")
    }

    /// Feeds one notice through the same function production uses.
    fn apply(snapshot: &mut PsiphonSnapshot, json: &str) -> Option<String> {
        apply_notice_to_snapshot(snapshot, &notice(json))
    }

    #[test]
    fn a_listener_without_a_tunnel_is_not_connected() {
        // The trap this carrier shares with tor: the SOCKS port is bound well
        // before anything can be carried through it. Reporting connected here
        // hands the chain a proxy with nowhere to forward to, which swallows
        // packets rather than refusing them.
        let mut snapshot = PsiphonSnapshot::default();
        apply(&mut snapshot, r#"{"noticeType":"ListeningSocksProxyPort","data":{"port":64347}}"#);
        assert_eq!(snapshot.socks_port, Some(64347));
        assert_ne!(snapshot.state, "connected");
    }

    #[test]
    fn one_tunnel_is_connected() {
        // Upstream's own comment says "when count > 1, the core is connected",
        // which is an off-by-one in their documentation. Following it literally
        // would leave a perfectly good single-tunnel session reported as still
        // connecting, forever.
        let mut snapshot = PsiphonSnapshot::default();
        apply(&mut snapshot, r#"{"noticeType":"ListeningSocksProxyPort","data":{"port":64347}}"#);
        apply(&mut snapshot, r#"{"noticeType":"Tunnels","data":{"count":1}}"#);
        assert_eq!(snapshot.state, "connected");
    }

    #[test]
    fn losing_every_tunnel_is_a_reconnect_and_not_a_failure() {
        // Psiphon reconnects on its own and keeps the listener bound while it
        // does. Tearing the chain down here would turn a reconnection it
        // handles into an outage we caused.
        let mut snapshot = PsiphonSnapshot::default();
        apply(&mut snapshot, r#"{"noticeType":"Tunnels","data":{"count":1}}"#);
        apply(&mut snapshot, r#"{"noticeType":"Tunnels","data":{"count":0}}"#);
        assert_eq!(snapshot.state, "connecting");
        assert_eq!(snapshot.status_message.as_deref(), Some("Reconnecting"));
        assert!(snapshot.last_error.is_none(), "a reconnect is not an error");
    }

    #[test]
    fn the_regions_come_from_psiphon_rather_than_from_a_table_of_ours() {
        // Measured: it reported 25. A hardcoded list goes stale silently and
        // offers countries that are no longer there.
        let mut snapshot = PsiphonSnapshot::default();
        apply(
            &mut snapshot,
            r#"{"noticeType":"AvailableEgressRegions","data":{"regions":["US","JP","FR","XYZ",""]}}"#,
        );
        assert_eq!(snapshot.available_regions, vec!["FR", "JP", "US"]);
    }

    #[test]
    fn the_exit_country_is_read_from_the_connected_server() {
        let mut snapshot = PsiphonSnapshot::default();
        apply(&mut snapshot, r#"{"noticeType":"ConnectedServerRegion","data":{"serverRegion":"FR"}}"#);
        assert_eq!(snapshot.exit_region.as_deref(), Some("FR"));
    }

    #[test]
    fn giving_up_names_the_exit_country_as_the_likely_reason() {
        // Measured: a fresh data directory holds 430 bootstrap servers, and
        // asking for Japan narrowed that to 13, none of which answered inside
        // the two-minute establish timeout. The process then exits, and without
        // reading this notice the only thing left to report is that it stopped
        // -- which points at the network rather than at the one setting that
        // actually caused it.
        let mut snapshot = PsiphonSnapshot::default();
        apply(&mut snapshot, r#"{"noticeType":"EstablishTunnelTimeout","data":{"timeout":"2m0s"}}"#);
        assert_eq!(snapshot.state, "error");
        let reason = snapshot.last_error.expect("a reason");
        assert!(reason.contains("exit country"), "{reason}");
        assert!(reason.contains("Best available"), "say what to do instead: {reason}");
    }

    #[test]
    fn a_region_is_either_two_upper_case_letters_or_nothing_at_all() {
        assert!(PsiphonSettings { egress_region: String::new(), ..Default::default() }.validate().is_ok());
        assert!(PsiphonSettings { egress_region: "JP".into(), ..Default::default() }.validate().is_ok());
        for bad in ["jp", "JPN", "J", "!!"] {
            assert!(
                PsiphonSettings { egress_region: bad.into(), ..Default::default() }.validate().is_err(),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn the_config_asks_for_a_chosen_port_and_no_http_listener() {
        let rendered = render_config(
            &PsiphonSettings { egress_region: "JP".into(), ..Default::default() },
            None,
            DEFAULT_SIGNATURE_PUBLIC_KEY,
            true, // chained mode: random port
        );
        let config: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        // Zero is "pick one and tell me" in chained mode.
        assert_eq!(config["LocalSocksProxyPort"], 0);
        assert_eq!(config["DisableLocalHTTPProxy"], true);
        assert_eq!(config["EgressRegion"], "JP");
        // Verify DataRootDirectory is NOT present in config JSON
        assert!(config.get("DataRootDirectory").is_none(), "DataRootDirectory must not be in config");
        assert_eq!(
            config["EstablishTunnelTimeoutSeconds"],
            ESTABLISH_TUNNEL_TIMEOUT_SECONDS
        );
        assert_eq!(config["EmitDiagnosticNotices"], true);
        assert_eq!(
            config["ServerEntrySignaturePublicKey"],
            DEFAULT_SIGNATURE_PUBLIC_KEY
        );
        assert_eq!(
            DEFAULT_SIGNATURE_PUBLIC_KEY.len(),
            44,
            "a base64 Ed25519 public key, with no stray whitespace from the file"
        );
    }

    #[test]
    fn standalone_port_defaults_to_10808_or_custom_port() {
        let rendered = render_config(
            &PsiphonSettings::default(),
            None,
            DEFAULT_SIGNATURE_PUBLIC_KEY,
            false, // standalone mode
        );
        let config: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(config["LocalSocksProxyPort"], DEFAULT_STANDALONE_PORT);
        assert_eq!(config["LocalSocksProxyListenInterface"], "127.0.0.1");

        let custom = render_config(
            &PsiphonSettings {
                listen_address: Some("0.0.0.0".into()),
                listen_port: Some(20808),
                ..Default::default()
            },
            None,
            DEFAULT_SIGNATURE_PUBLIC_KEY,
            false,
        );
        let custom_config: serde_json::Value = serde_json::from_str(&custom).unwrap();
        assert_eq!(custom_config["LocalSocksProxyPort"], 20808);
        assert_eq!(custom_config["LocalSocksProxyListenInterface"], "0.0.0.0");
    }

    #[test]
    fn a_hop_in_front_is_named_and_its_absence_is_silence() {
        let chained = render_config(
            &PsiphonSettings::default(),
            Some("127.0.0.1:1819".parse().unwrap()),
            DEFAULT_SIGNATURE_PUBLIC_KEY,
            true,
        );
        let config: serde_json::Value = serde_json::from_str(&chained).unwrap();
        assert_eq!(config["UpstreamProxyURL"], "socks5://127.0.0.1:1819");

        let alone = render_config(
            &PsiphonSettings::default(),
            None,
            DEFAULT_SIGNATURE_PUBLIC_KEY,
            false,
        );
        let config: serde_json::Value = serde_json::from_str(&alone).unwrap();
        assert!(config.get("UpstreamProxyURL").is_none(), "{alone}");
    }

    #[test]
    fn no_exit_country_is_sent_as_empty_rather_than_omitted() {
        let rendered = render_config(
            &PsiphonSettings::default(),
            None,
            DEFAULT_SIGNATURE_PUBLIC_KEY,
            true,
        );
        let config: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(config["EgressRegion"], "");
    }

    #[test]
    fn a_notice_that_is_not_json_never_stops_the_reader() {
        assert!(serde_json::from_str::<Notice>("panic: runtime error").is_err());
    }

    #[test]
    fn counts_server_entries_accurately() {
        let temp_dir = std::env::temp_dir().join("nextvpn_psiphon_test");
        let _ = std::fs::create_dir_all(&temp_dir);
        let list_file = temp_dir.join("test_server_entries.txt");
        let content = "# Comment line\n\nentry_hex_1\nentry_hex_2\n   \n# Another comment\nentry_hex_3\n";
        std::fs::write(&list_file, content).unwrap();
        assert_eq!(count_server_entries(&list_file), 3);
        let _ = std::fs::remove_file(&list_file);
    }

    #[test]
    fn custom_channel_and_sponsor_ids_are_rendered() {
        let settings = PsiphonSettings {
            propagation_channel_id: Some("CUSTOM_CHANNEL_123".into()),
            sponsor_id: Some("CUSTOM_SPONSOR_456".into()),
            ..Default::default()
        };
        let rendered = render_config(&settings, None, DEFAULT_SIGNATURE_PUBLIC_KEY, false);
        let config: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(config["PropagationChannelId"], "CUSTOM_CHANNEL_123");
        assert_eq!(config["SponsorId"], "CUSTOM_SPONSOR_456");
    }

    #[test]
    fn standalone_configuration_saves_and_loads_independently() {
        let temp_dir = std::env::temp_dir().join(format!("psiphon_cfg_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let cfg_file = temp_dir.join("standalone_config.json");

        let settings = PsiphonSettings {
            egress_region: "US".into(),
            signature_public_key: Some("custom_test_key".into()),
            listen_address: Some("0.0.0.0".into()),
            listen_port: Some(19876),
            propagation_channel_id: Some("CHAN_TEST".into()),
            sponsor_id: Some("SPON_TEST".into()),
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&settings).unwrap();
        std::fs::write(&cfg_file, json).unwrap();

        let loaded: PsiphonSettings = serde_json::from_str(&std::fs::read_to_string(&cfg_file).unwrap()).unwrap();
        assert_eq!(loaded, settings);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

