use std::{
    io::{BufRead, BufReader, Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use crate::app_context::AppContext;
use super::types::{CoreLogEvent, CoreSnapshot, MAX_LOGS};
use super::supervisor::{lock, CoreSupervisor, SupervisorInner};

pub const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn mark_snapshot_dirty(inner: &SupervisorInner) {
    inner.snapshot_dirty.store(true, Ordering::SeqCst);
}

pub fn push_log(inner: &SupervisorInner, stream: &str, level: &str, message: String) {
    let event = CoreLogEvent {
        timestamp: now_millis(),
        stream: stream.into(),
        level: level.into(),
        message,
    };
    {
        let mut logs = lock(&inner.logs);
        if logs.len() == MAX_LOGS {
            logs.pop_front();
        }
        logs.push_back(event.clone());
    }
    let mut pending = lock(&inner.pending);
    if pending.len() < MAX_LOGS {
        pending.push(event);
    }
}

pub fn supervisor_log(inner: &SupervisorInner, level: &str, message: String) {
    push_log(inner, "supervisor", level, message);
}

pub fn record_log(_app: &AppContext, inner: &SupervisorInner, stream: &str, message: String) {
    let message = message.trim().to_string();
    push_log(inner, stream, log_level(&message), message.clone());

    let engine_owns_route = lock(&inner.session)
        .as_ref()
        .is_none_or(|session| session.profile.carriers.is_lone_aether());

    let connected = {
        let mut snapshot = lock(&inner.snapshot);
        if snapshot.pid.is_none() {
            false
        } else {
            let before = snapshot.clone();
            apply_log_to_snapshot(&message, &mut snapshot, engine_owns_route);
            let connected = snapshot.state == "connected";
            if connected {
                snapshot.attempt = 0;
                snapshot.status_message = None;
            }
            if *snapshot != before {
                mark_snapshot_dirty(inner);
            }
            connected
        }
    };

    if !connected && sweep_exhausted(&message) {
        end_fruitless_sweep(inner);
    }
}

pub fn spawn_log_reader<R: Read + Send + 'static>(
    app: AppContext,
    inner: Arc<SupervisorInner>,
    reader: R,
    stream: &'static str,
) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => record_log(&app, &inner, stream, line),
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => continue,
                Err(_) => break,
            }
        }
    });
}

pub fn sweep_exhausted(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    lowered.contains("rescanning shortly")
        || lowered.contains("no usable masque gateway found")
        || lowered.contains("scan deadline reached with no gateway")
}

pub fn end_fruitless_sweep(inner: &SupervisorInner) {
    let mut guard = lock(&inner.child);
    let Some(child) = guard.as_mut() else {
        return;
    };
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = child.kill();
    drop(guard);
    supervisor_log(
        inner,
        "warn",
        "a full sweep found no gateway; trying the other transport rather than repeating it".into(),
    );
}

pub fn apply_log_to_snapshot(message: &str, snapshot: &mut CoreSnapshot, engine_owns_route: bool) {
    if message.contains("hunting for a working") || message.contains("verifying cached") {
        snapshot.state = "scanning".into();
    }
    if message.contains("MASQUE transport: HTTP/2") {
        snapshot.transport = Some("masque-h2".into());
        snapshot.state = "connecting".into();
    } else if message.contains("MASQUE transport: HTTP/3") {
        snapshot.transport = Some("masque-h3".into());
        snapshot.state = "connecting".into();
    } else if message.contains("validating WireGuard tunnel") {
        snapshot.transport = Some("wireguard".into());
        snapshot.state = "connecting".into();
    }
    if let Some(endpoint) = parse_endpoint(message) {
        snapshot.endpoint = Some(endpoint);
    }
    if let Some(latency) = parse_latency_ms(message) {
        snapshot.latency_ms = Some(latency);
    }

    const LISTEN_GATES: [&str; 2] = ["socks5 server listening on ", "socks5 listening on "];
    const FAILURE_MARKERS: [&str; 7] = [
        "failed", "could not", "cannot", "unable to", "refused", "already in use", "error",
    ];
    let lowered = message.to_ascii_lowercase();
    if !FAILURE_MARKERS.iter().any(|marker| lowered.contains(marker)) {
        for gate in LISTEN_GATES {
            let Some(rest) = message.split(gate).nth(1) else {
                continue;
            };
            if let Some(candidate) = rest.split_whitespace().next() {
                if candidate.parse::<SocketAddr>().is_ok() {
                    if engine_owns_route {
                        snapshot.socks_address = candidate.to_string();
                    }
                    snapshot.state = "connected".into();
                    snapshot.blocking = false;
                }
            }
            break;
        }
    }
    if message.contains("reconnecting") {
        snapshot.state = "reconnecting".into();
    }
    if message.contains(" ERROR ") || message.starts_with("ERROR") {
        snapshot.last_error = Some(strip_logger_prefix(message));
    }
}

pub fn parse_endpoint(message: &str) -> Option<String> {
    const MARKERS: [&str; 5] = [
        "selected MASQUE gateway ",
        "selected WireGuard endpoint ",
        "using cloudflare edge ",
        "cached gateway ",
        "cached endpoint ",
    ];
    for marker in MARKERS {
        let Some(rest) = message.split(marker).nth(1) else {
            continue;
        };
        let candidate = rest
            .split_whitespace()
            .next()?
            .trim_end_matches(|c| c == ',' || c == ')');
        if candidate.parse::<SocketAddr>().is_ok() {
            return Some(candidate.into());
        }
    }
    None
}

pub fn parse_latency_ms(message: &str) -> Option<f64> {
    let rest = message.split("rtt ").nth(1)?;
    let token = rest
        .split_whitespace()
        .next()?
        .trim_end_matches(|c| c == ')' || c == ',');
    if let Some(value) = token.strip_suffix("ms") {
        return value.parse().ok();
    }
    if let Some(value) = token.strip_suffix('s') {
        return value.parse::<f64>().ok().map(|seconds| seconds * 1_000.0);
    }
    None
}

pub fn log_level(message: &str) -> &'static str {
    if message.contains(" ERROR ") || message.starts_with("ERROR") {
        "error"
    } else if message.contains(" WARN ") || message.starts_with("WARN") {
        "warn"
    } else if message.contains(" DEBUG ") || message.starts_with("DEBUG") {
        "debug"
    } else if message.contains(" TRACE ") || message.starts_with("TRACE") {
        "trace"
    } else {
        "info"
    }
}

pub fn strip_logger_prefix(message: &str) -> String {
    message
        .splitn(2, " - ")
        .nth(1)
        .unwrap_or(message)
        .trim()
        .to_string()
}

pub fn start_pump(app: AppContext, supervisor: &CoreSupervisor) {
    let inner = supervisor.inner.clone();
    let log_path = session_log_path(&app);
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(120));

        let batch: Vec<CoreLogEvent> = {
            let mut pending = lock(&inner.pending);
            if pending.is_empty() {
                Vec::new()
            } else {
                std::mem::take(&mut *pending)
            }
        };
        if !batch.is_empty() {
            if let Some(path) = log_path.as_deref() {
                append_session_log(path, &batch);
            }
            let _ = app.emit("core-logs", &batch);
        }
        if inner.snapshot_dirty.swap(false, Ordering::SeqCst) {
            let snapshot = lock(&inner.snapshot).clone();
            let _ = app.emit("core-status", &snapshot);
        }
    });
}

pub fn session_log_path(app: &AppContext) -> Option<PathBuf> {
    let dir = app.path().app_log_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("core.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(&path, dir.join("core.previous.log"));
    }
    Some(path)
}

pub fn append_session_log(path: &Path, batch: &[CoreLogEvent]) {
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let mut text = String::new();
    for event in batch {
        text.push_str(&format!(
            "{} [{}/{}] {}\n",
            event.timestamp, event.stream, event.level, event.message
        ));
    }
    let _ = file.write_all(text.as_bytes());
}
