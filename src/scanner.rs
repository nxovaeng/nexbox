use std::sync::Arc;
use crate::app_context::AppContext;
// Asks the core which gateways are reachable, without connecting to one.
//
// The core answers `--scan-only` and `--test-endpoint` with a single JSON
// object on stdout and exits. Running it as a child rather than linking the
// engine keeps this crate free of the C++ toolchain BoringSSL needs, and keeps
// the connection path — which supports protocols the embedded API does not —
// exactly as it was.

use crate::core_supervisor::{CoreProfile, CoreSupervisor};
use serde::Serialize;
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

/// The core's own cap; asking for more is silently reduced, so mirror it here.
const MAX_CANDIDATES: u32 = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCandidate {
    pub peer: String,
    pub rtt_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub candidates: Vec<ScanCandidate>,
    /// Which transport actually produced them, which is not always the one asked for.
    pub transport: String,
    /// Set when the first transport found nothing and the other was swept instead.
    pub fell_back: bool,
}


pub async fn scan_endpoints(
    app: AppContext,
    supervisor: Arc<CoreSupervisor>,
    profile: CoreProfile,
    limit: u32,
) -> Result<ScanOutcome, String> {
    supervisor.require_idle("Disconnect before scanning for endpoints")?;
    let inner: CoreSupervisor = (*supervisor).clone();
    let limit = limit.clamp(1, MAX_CANDIDATES);
    tokio::task::spawn_blocking(move || scan_blocking(&app, &inner, &profile, limit))
        .await
        .map_err(|error| format!("the scan did not finish: {error}"))?
}


pub async fn test_endpoint(
    app: AppContext,
    supervisor: Arc<CoreSupervisor>,
    profile: CoreProfile,
    endpoint: String,
) -> Result<ScanCandidate, String> {
    supervisor.require_idle("Disconnect before testing an endpoint")?;
    let inner: CoreSupervisor = (*supervisor).clone();
    tokio::task::spawn_blocking(move || {
        // The transport the profile is set to, not a fixed one. A gateway can
        // answer over TCP and be unusable over QUIC -- on a network that
        // interferes with UDP, every H3 endpoint fails while H2 succeeds -- so
        // testing the wrong transport reports an endpoint as good that the next
        // connect cannot use.
        let transport = transport_of(&profile);
        let value = run_report(
            &app,
            &inner,
            &profile,
            &["--test-endpoint".into(), endpoint],
            transport,
        )?;
        candidate_from(&value).ok_or_else(|| "the core gave an unreadable answer".to_string())
    })
    .await
    .map_err(|error| format!("the test did not finish: {error}"))?
}


pub fn cancel_scan(supervisor: Arc<CoreSupervisor>) -> bool {
    supervisor.cancel_scan()
}

/// Which MASQUE transport the profile is set to.
///
/// The scan and the endpoint test both need this: probing over the wrong one
/// answers a question the user did not ask.
fn transport_of(profile: &CoreProfile) -> &'static str {
    if profile.masque_transport == "h2" { "h2" } else { "h3" }
}

fn scan_blocking(
    app: &AppContext,
    supervisor: &CoreSupervisor,
    profile: &CoreProfile,
    limit: u32,
) -> Result<ScanOutcome, String> {
    let configured = transport_of(profile);
    let args = vec!["--scan-only".to_string(), "--scan-limit".into(), limit.to_string()];

    let first = run_report(app, supervisor, profile, &args, configured)?;
    let candidates = candidates_from(&first);
    if !candidates.is_empty() {
        return Ok(ScanOutcome { candidates, transport: configured.into(), fell_back: false });
    }

    // The prober picks its socket from the transport it is handed: H2 probes
    // over TCP, H3 over QUIC. Scanning only the configured one therefore
    // searches UDP alone on the default profile, and a network that blocks UDP
    // reports an empty internet. Sweep the other before believing that.
    let other = if configured == "h2" { "h3" } else { "h2" };
    let second = run_report(app, supervisor, profile, &args, other)?;
    Ok(ScanOutcome {
        candidates: candidates_from(&second),
        transport: other.into(),
        fell_back: true,
    })
}

/// Runs the core in a reporting mode and returns the JSON object it printed.
fn run_report(
    app: &AppContext,
    supervisor: &CoreSupervisor,
    profile: &CoreProfile,
    extra: &[String],
    transport: &str,
) -> Result<serde_json::Value, String> {
    let (core_path, config_dir, identity_path) = crate::core_supervisor::core_paths(app, profile)?;

    let mut command = Command::new(&core_path);
    command
        // MASQUE regardless of the profile's protocol: the core's reporting
        // modes wrap scan_embedded, which has no WireGuard or WARP-in-WARP
        // path. The panel says so rather than quietly scanning the wrong thing.
        .arg("--masque")
        .args(["--scan", &profile.scan_mode])
        .args(["--ip", &profile.ip_family])
        .args(["--noize", &profile.noize])
        .args(["--config", &identity_path.to_string_lossy()])
        .args(["--log-level", "warn"])
        .args(extra)
        .current_dir(&config_dir)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if transport == "h2" {
        command.arg("--h2");
    }
    crate::core_supervisor::hide_console(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run the core: {error}"))?;
    let stdout = child.stdout.take();

    supervisor.hold_scan(child)?;
    let reader = thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stdout) = stdout {
            let _ = stdout.read_to_string(&mut text);
        }
        text
    });

    // Polled rather than waited on, so a cancel can take the child out from
    // under this loop and kill it.
    loop {
        thread::sleep(Duration::from_millis(150));
        match supervisor.poll_scan() {
            ScanState::Gone => return Err("the scan was cancelled".into()),
            ScanState::Running => continue,
            ScanState::Exited => break,
        }
    }

    let text = reader.join().unwrap_or_default();
    let line = text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| "the core reported nothing".to_string())?;
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|error| format!("the core gave an unreadable answer: {error}"))?;

    if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("the scan failed")
            .to_string());
    }
    Ok(value)
}

pub enum ScanState {
    Running,
    Exited,
    /// Taken by a cancel.
    Gone,
}

fn candidates_from(value: &serde_json::Value) -> Vec<ScanCandidate> {
    value
        .get("results")
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().filter_map(candidate_from).collect())
        .unwrap_or_default()
}

fn candidate_from(value: &serde_json::Value) -> Option<ScanCandidate> {
    Some(ScanCandidate {
        peer: value.get("peer")?.as_str()?.to_string(),
        rtt_ms: value.get("rttMs")?.as_u64()?,
    })
}
