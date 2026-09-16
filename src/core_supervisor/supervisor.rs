use std::{
    collections::VecDeque,
    process::Child,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
};
use crate::app_context::AppContext;
use crate::carrier::{Carrier, RunningChain};
use crate::http_bridge::HttpBridge;
use super::types::{CoreLogEvent, CoreProbe, CoreProfile, CoreSnapshot, ProxyRoute, Session, MAX_LOGS, MAX_REPORT_BYTES};
use super::logs::{mark_snapshot_dirty, push_log, supervisor_log};
use super::process::{core_version, launch, resolve_core_path};
use super::carrier::{current_carrier, current_chain, start_carrier_blocking, stop_all_hops, stop_carriers};

pub struct SupervisorInner {
    pub child: Mutex<Option<Child>>,
    pub snapshot: Mutex<CoreSnapshot>,
    pub logs: Mutex<VecDeque<CoreLogEvent>>,
    pub session: Mutex<Option<Session>>,
    pub chain: Mutex<Option<RunningChain>>,
    pub generation: AtomicU64,
    pub proxy_route: Mutex<Option<ProxyRoute>>,
    pub scan_child: Mutex<Option<Child>>,
    pub bridge: Mutex<Option<HttpBridge>>,
    pub pending: Mutex<Vec<CoreLogEvent>>,
    pub snapshot_dirty: AtomicBool,
}

impl SupervisorInner {
    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub async fn off_thread<T, F>(what: &str, body: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(body).await {
        Ok(result) => result,
        Err(error) => Err(format!("{what} did not finish: {error}")),
    }
}

#[derive(Clone)]
pub struct CoreSupervisor {
    pub(crate) inner: Arc<SupervisorInner>,
}

impl CoreSupervisor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SupervisorInner {
                child: Mutex::new(None),
                snapshot: Mutex::new(CoreSnapshot::default()),
                logs: Mutex::new(VecDeque::with_capacity(MAX_LOGS)),
                session: Mutex::new(None),
                chain: Mutex::new(None),
                generation: AtomicU64::new(0),
                proxy_route: Mutex::new(None),
                scan_child: Mutex::new(None),
                bridge: Mutex::new(None),
                pending: Mutex::new(Vec::new()),
                snapshot_dirty: AtomicBool::new(false),
            }),
        }
    }

    pub fn snapshot(&self) -> CoreSnapshot {
        lock(&self.inner.snapshot).clone()
    }

    pub fn status(&self) -> CoreSnapshot {
        self.snapshot()
    }

    pub fn logs(&self) -> Vec<CoreLogEvent> {
        lock(&self.inner.logs).iter().cloned().collect()
    }

    pub fn connected_socks(&self) -> Option<String> {
        let snapshot = lock(&self.inner.snapshot);
        (snapshot.state == "connected").then(|| snapshot.socks_address.clone())
    }

    pub fn carries_quic(&self, app: &AppContext) -> bool {
        match current_chain(app, &self.inner) {
            Some(chain) => chain.carries_quic(),
            None => true,
        }
    }

    pub fn carrier(&self, app: &AppContext) -> Option<Carrier> {
        current_carrier(app, &self.inner)
    }

    pub fn chain(&self, app: &AppContext) -> Option<RunningChain> {
        current_chain(app, &self.inner)
    }

    pub fn record(&self, stream: &str, level: &str, message: String) {
        push_log(&self.inner, stream, level, message);
    }

    pub fn require_idle(&self, message: &str) -> Result<(), String> {
        let state = lock(&self.inner.snapshot).state.clone();
        if state == "idle" || state == "stopped" || state == "error" {
            Ok(())
        } else {
            Err(message.to_string())
        }
    }

    pub fn hold_scan(&self, child: Child) -> Result<(), String> {
        let mut guard = lock(&self.inner.scan_child);
        if guard.is_some() {
            return Err("a scan is already running".into());
        }
        *guard = Some(child);
        Ok(())
    }

    pub fn poll_scan(&self) -> crate::scanner::ScanState {
        let mut guard = lock(&self.inner.scan_child);
        let Some(child) = guard.as_mut() else {
            return crate::scanner::ScanState::Gone;
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                guard.take();
                crate::scanner::ScanState::Exited
            }
            Ok(None) => crate::scanner::ScanState::Running,
            Err(_) => {
                guard.take();
                crate::scanner::ScanState::Exited
            }
        }
    }

    pub fn cancel_scan(&self) -> bool {
        let Some(mut child) = lock(&self.inner.scan_child).take() else {
            return false;
        };
        let _ = child.kill();
        let _ = child.wait();
        true
    }

    pub fn shutdown(&self, app: &AppContext) {
        self.cancel_scan();
        let _ = stop_inner(&self.inner, app);
    }
}

pub fn runtime_info() -> serde_json::Value {
    serde_json::json!({
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "pid": std::process::id(),
    })
}

pub fn core_status(supervisor: Arc<CoreSupervisor>) -> CoreSnapshot {
    supervisor.snapshot()
}

pub fn core_logs(supervisor: Arc<CoreSupervisor>) -> Vec<CoreLogEvent> {
    supervisor.logs()
}

pub async fn probe_core(app: AppContext, profile: Option<CoreProfile>) -> CoreProbe {
    off_thread("the core check", move || Ok(probe_core_blocking(&app, profile)))
        .await
        .unwrap_or_else(|error| CoreProbe {
            available: false,
            path: None,
            version: None,
            message: error,
        })
}

pub fn probe_core_blocking(app: &AppContext, profile: Option<CoreProfile>) -> CoreProbe {
    let requested = profile.and_then(|value| value.core_path);
    match resolve_core_path(app, requested.as_deref()) {
        Ok(path) => match core_version(&path) {
            Ok(version) => CoreProbe {
                available: true,
                path: Some(path.to_string_lossy().into_owned()),
                version: Some(version),
                message: "Core is ready".into(),
            },
            Err(error) => CoreProbe {
                available: false,
                path: Some(path.to_string_lossy().into_owned()),
                version: None,
                message: error,
            },
        },
        Err(error) => CoreProbe {
            available: false,
            path: None,
            version: None,
            message: error,
        },
    }
}

pub async fn start_core(
    app: AppContext,
    supervisor: Arc<CoreSupervisor>,
    profile: CoreProfile,
) -> Result<CoreSnapshot, String> {
    let inner = supervisor.inner.clone();
    off_thread("starting the core", move || start_core_blocking(&app, &inner, profile)).await
}

pub fn start_core_blocking(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    profile: CoreProfile,
) -> Result<CoreSnapshot, String> {
    profile.validate()?;
    if lock(&inner.child).is_some() || lock(&inner.session).is_some() {
        return Err("Aether core is already running".into());
    }

    let generation = inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
    *lock(&inner.session) = Some(Session {
        generation,
        profile: profile.clone(),
        attempt: 0,
    });

    supervisor_log(
        inner,
        "info",
        format!("the way out is {}", profile.carriers.label()),
    );

    if !profile.carriers.is_lone_aether() {
        return match start_carrier_blocking(app, inner, &profile, generation) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                *lock(&inner.session) = None;
                stop_all_hops(app, inner);
                let mut snapshot = lock(&inner.snapshot);
                snapshot.state = "error".into();
                snapshot.pid = None;
                snapshot.status_message = None;
                snapshot.last_error = Some(error.clone());
                mark_snapshot_dirty(inner);
                Err(error)
            }
        };
    }

    match launch(app, inner, &profile, 0, generation, true) {
        Ok(()) => Ok(lock(&inner.snapshot).clone()),
        Err(error) => {
            *lock(&inner.session) = None;
            Err(error)
        }
    }
}

pub async fn stop_core(app: AppContext, supervisor: Arc<CoreSupervisor>) -> Result<(), String> {
    let inner = supervisor.inner.clone();
    off_thread("stopping the core", move || stop_inner(&inner, &app)).await
}

pub fn stop_inner(inner: &SupervisorInner, app: &AppContext) -> Result<(), String> {
    app.chain().stop();
    app.lan_door().close();
    stop_carriers(app);
    inner.generation.fetch_add(1, Ordering::SeqCst);
    *lock(&inner.session) = None;

    let mut child = lock(&inner.child).take();
    if let Some(child) = child.as_mut() {
        child
            .kill()
            .map_err(|error| format!("failed to stop Aether core: {error}"))?;
        let _ = child.wait();
    }
    let mut snapshot = lock(&inner.snapshot);
    snapshot.state = "idle".into();
    snapshot.pid = None;
    snapshot.transport = None;
    snapshot.endpoint = None;
    snapshot.latency_ms = None;
    snapshot.started_at = None;
    snapshot.last_error = None;
    snapshot.status_message = None;
    snapshot.attempt = 0;
    mark_snapshot_dirty(inner);
    Ok(())
}

pub fn is_available(app: &AppContext) -> bool {
    resolve_core_path(app, None).is_ok()
}

pub async fn save_report(
    app: AppContext,
    contents: String,
    filename: String,
) -> Result<String, String> {
    off_thread("saving the report", move || {
        save_report_blocking(&app, contents, filename)
    })
    .await
}

pub fn save_report_blocking(
    app: &AppContext,
    contents: String,
    filename: String,
) -> Result<String, String> {
    if contents.trim().is_empty() {
        return Err("the report is empty".into());
    }
    if contents.len() > MAX_REPORT_BYTES {
        return Err("the report is too large to save".into());
    }
    let name = sanitize_report_name(&filename)?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve app data directory: {error}"))?
        .join("reports");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create the reports directory: {error}"))?;
    let path = directory.join(name);
    std::fs::write(&path, contents).map_err(|error| format!("cannot save the report: {error}"))?;
    Ok(path.to_string_lossy().into_owned())
}

pub fn sanitize_report_name(filename: &str) -> Result<String, String> {
    let name = filename.trim();
    if name.is_empty() || name.len() > 128 {
        return Err("the report file name is invalid".into());
    }
    if !name.ends_with(".txt") {
        return Err("reports are saved as .txt".into());
    }
    if name.starts_with('.')
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err("the report file name is invalid".into());
    }
    Ok(name.to_string())
}
