use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};
use crate::app_context::AppContext;
use super::types::{
    non_empty, CoreProfile, CoreSnapshot, ExitDecision, Session, BASE_RETRY_SECS,
    MAX_ATTEMPTS, MAX_RETRY_SECS, VERSION_PROBE_TIMEOUT,
};
use super::supervisor::{lock, SupervisorInner};
use super::logs::{mark_snapshot_dirty, now_millis, supervisor_log};
use super::system_route::proxy_is_applied;

const HOLD_RETRY: Duration = Duration::from_secs(30);

pub fn hide_console(command: &mut Command) {
    hide_console_window(command);
}

#[cfg(windows)]
pub fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console_window(_command: &mut Command) {}

pub fn set_optional_env(command: &mut Command, key: &str, value: Option<&str>) {
    if let Some(value) = non_empty(value) {
        command.env(key, value);
    }
}

pub fn core_filename() -> &'static str {
    if cfg!(windows) {
        "aether.exe"
    } else {
        "aether"
    }
}

pub fn resolve_core_path(app: &AppContext, requested: Option<&str>) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(data_dir) = app.path().app_data_dir() {
        candidates.push(data_dir.join("aether").join(core_filename()));
        candidates.push(data_dir.join("bin").join(core_filename()));
        candidates.push(data_dir.join(core_filename()));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("data").join("aether").join(core_filename()));
        candidates.push(cwd.join("nextvpn").join("data").join("aether").join(core_filename()));
        candidates.push(cwd.join("data").join("bin").join(core_filename()));
        candidates.push(cwd.join("nextvpn").join("data").join("bin").join(core_filename()));
        candidates.push(cwd.join("data").join(core_filename()));
        candidates.push(cwd.join("nextvpn").join("data").join(core_filename()));
    }
    if let Some(path) = non_empty(requested) {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(path) = std::env::var("WHITEAESTHER_CORE_PATH") {
        if !path.trim().is_empty() {
            candidates.push(PathBuf::from(path));
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(core_filename()));
        candidates.push(resource_dir.join("binaries").join(core_filename()));
    }
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.join(core_filename()));
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join(core_filename()));
        candidates.push(
            current_dir
                .join("..")
                .join("Aether")
                .join("aether")
                .join("target")
                .join("debug")
                .join(core_filename()),
        );
    }

    for candidate in candidates {
        if !candidate.is_file() {
            continue;
        }
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        let filename = canonical
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if filename == "aether" || filename.starts_with("aether-") {
            return Ok(canonical);
        }
    }

    Err("Aether core not found".into())
}

pub fn core_version(path: &Path) -> Result<String, String> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("RUST_LOG");
    hide_console_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run Aether core: {error}"))?;

    let stdout = child.stdout.take().expect("stdout is piped above");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.take(64 * 1024).read_to_end(&mut buffer);
        let _ = sender.send(buffer);
    });

    let Ok(buffer) = receiver.recv_timeout(VERSION_PROBE_TIMEOUT) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Aether version check timed out".into());
    };
    let status = child
        .wait()
        .map_err(|error| format!("cannot run Aether core: {error}"))?;
    if !status.success() {
        return Err(format!("Aether version check failed with {status}"));
    }

    let version = String::from_utf8_lossy(&buffer).trim().to_string();
    if !version.to_ascii_lowercase().starts_with("aether ") {
        return Err("the selected executable is not an Aether core".into());
    }
    Ok(version)
}

pub fn core_paths(
    app: &AppContext,
    profile: &CoreProfile,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let core_path = resolve_core_path(app, profile.core_path.as_deref())?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve app data directory: {error}"))?;
    let identity_dir = data_dir.join("identity");
    std::fs::create_dir_all(&identity_dir)
        .map_err(|error| format!("cannot create identity directory: {error}"))?;
    let aether_dir = core_path.parent().unwrap_or(&data_dir).to_path_buf();
    let aether_identity = aether_dir.join("aether.toml");
    let id_file = if aether_identity.exists() {
        aether_identity
    } else {
        identity_dir.join("aether.toml")
    };
    Ok((core_path, aether_dir, id_file))
}

pub fn launch(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    profile: &CoreProfile,
    attempt: u32,
    generation: u64,
    supervised: bool,
) -> Result<(), String> {
    let core_path = resolve_core_path(app, profile.core_path.as_deref())?;
    let version = core_version(&core_path)?;
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve app data directory: {error}"))?;
    let identity_dir = data_dir.join("identity");
    std::fs::create_dir_all(&identity_dir)
        .map_err(|error| format!("cannot create identity directory: {error}"))?;

    let mut effective_profile = profile.clone();
    let has_routing_rules = profile.bypass_direct_routes
        || !profile.route_block.trim().is_empty()
        || !profile.route_direct.trim().is_empty()
        || profile.routes_file.is_some()
        || crate::routing_rules::has_custom_rules(&data_dir);

    if has_routing_rules {
        let generated = crate::routing_rules::generated_routes_path(&data_dir);
        crate::routing_rules::write_combined_routes_file(
            &data_dir,
            &generated,
            profile.routes_file.as_deref().map(Path::new),
            Some(&profile.route_block),
            Some(&profile.route_direct),
        )?;
        effective_profile.routes_file = Some(generated.to_string_lossy().into_owned());
        effective_profile.route_block = String::new();
        effective_profile.route_direct = String::new();
    }

    let aether_dir = core_path.parent().unwrap_or(&data_dir);
    let aether_identity = aether_dir.join("aether.toml");
    let identity_path = if aether_identity.exists() {
        aether_identity
    } else {
        identity_dir.join("aether.toml")
    };

    let mut command = Command::new(&core_path);
    command
        .args(effective_profile.args(&identity_path))
        .current_dir(aether_dir)
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    set_optional_env(&mut command, "AETHER_TEAM", profile.team.as_deref());
    set_optional_env(
        &mut command,
        "AETHER_ACCESS_CLIENT_ID",
        profile.access_client_id.as_deref(),
    );
    set_optional_env(
        &mut command,
        "AETHER_ACCESS_CLIENT_SECRET",
        profile.access_client_secret.as_deref(),
    );
    set_optional_env(
        &mut command,
        "AETHER_ACCESS_EMAIL",
        profile.access_email.as_deref(),
    );
    set_optional_env(
        &mut command,
        "AETHER_ACCESS_TOKEN",
        profile.access_token.as_deref(),
    );

    set_optional_env(
        &mut command,
        "AETHER_UPSTREAM",
        non_empty(Some(profile.upstream_proxy.as_str())),
    );

    if !profile.route_sniff {
        command.env("AETHER_ROUTE_SNIFF", "0");
    }
    if !profile.auto_reprovision {
        command.env("AETHER_REPROVISION", "0");
    }

    hide_console_window(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start Aether core: {error}"))?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut guard = lock(&inner.child);
        if !inner.is_current(generation) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
        *guard = Some(child);
    }

    {
        let mut snapshot = lock(&inner.snapshot);
        *snapshot = CoreSnapshot {
            state: "starting".into(),
            pid: Some(pid),
            core_path: Some(core_path.to_string_lossy().into_owned()),
            version: Some(version),
            transport: Some(transport_label(profile).into()),
            endpoint: None,
            socks_address: profile.socks_address.clone(),
            latency_ms: None,
            started_at: Some(now_millis()),
            last_error: None,
            status_message: (attempt > 0).then(|| {
                format!(
                    "Attempt {attempt} of {MAX_ATTEMPTS} on {}",
                    transport_name(profile)
                )
            }),
            attempt,
            max_attempts: MAX_ATTEMPTS,
            blocking: false,
        };
        mark_snapshot_dirty(inner);
    }

    supervisor_log(inner, "info", session_summary(profile, attempt));

    if let Some(stdout) = stdout {
        super::logs::spawn_log_reader(app.clone(), inner.clone(), stdout, "stdout");
    }
    if let Some(stderr) = stderr {
        super::logs::spawn_log_reader(app.clone(), inner.clone(), stderr, "stderr");
    }

    if supervised {
        spawn_exit_monitor(app.clone(), inner.clone(), generation);
        super::system_route::spawn_route_watch(app.clone(), inner.clone(), generation);
    }

    Ok(())
}

pub fn spawn_exit_monitor(app: AppContext, inner: Arc<SupervisorInner>, generation: u64) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(350));
        if !inner.is_current(generation) {
            return;
        }
        let exit = {
            let mut guard = lock(&inner.child);
            let Some(child) = guard.as_mut() else {
                return;
            };
            match child.try_wait() {
                Ok(Some(status)) => {
                    guard.take();
                    Some(Ok(status))
                }
                Ok(None) => None,
                Err(error) => {
                    guard.take();
                    Some(Err(error.to_string()))
                }
            }
        };

        let Some(exit) = exit else { continue };
        let reason = match exit {
            Ok(status) if status.success() => "The Aether core exited".to_string(),
            Ok(status) => format!("The Aether core exited with {status}"),
            Err(error) => format!("Cannot monitor the Aether core: {error}"),
        };
        handle_exit(&app, &inner, generation, reason);
        return;
    });
}

pub fn decide_exit(session: &mut Option<Session>, generation: u64, holding: bool) -> ExitDecision {
    let Some(current) = session.as_mut() else {
        return ExitDecision::Ignore;
    };
    if current.generation != generation {
        return ExitDecision::Ignore;
    }
    current.attempt += 1;
    let attempt = current.attempt;
    let profile = current.profile.clone();
    if attempt > MAX_ATTEMPTS || !profile.auto_reconnect {
        if holding {
            current.attempt = 0;
            return ExitDecision::Hold { profile };
        }
        *session = None;
        return ExitDecision::GiveUp { profile };
    }
    ExitDecision::Retry { attempt, profile }
}

pub fn give_up(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    generation: u64,
    profile: &CoreProfile,
    reason: String,
) {
    let summary = if !profile.auto_reconnect {
        format!("{reason}. Not retried, because \"keep me connected\" is off.")
    } else if profile.endpoint_mode == "custom-only" {
        format!(
            "The pinned endpoint {} never answered. Stopped after {MAX_ATTEMPTS} attempts — switch Endpoint back to Automatic to search instead.",
            profile.peer.as_deref().unwrap_or("(unset)")
        )
    } else {
        format!("{reason}. Stopped after {MAX_ATTEMPTS} attempts.")
    };
    {
        let mut snapshot = lock(&inner.snapshot);
        if !inner.is_current(generation) {
            return;
        }
        snapshot.state = "error".into();
        snapshot.pid = None;
        snapshot.status_message = None;
        snapshot.attempt = 0;
        snapshot.last_error = Some(summary);
        mark_snapshot_dirty(inner);
    }
    supervisor_log(
        inner,
        "error",
        format!("gave up after {MAX_ATTEMPTS} attempts: {reason}"),
    );
    app.chain().stop();

    if profile.kill_switch && proxy_is_applied(inner) {
        supervisor_log(
            inner,
            "warn",
            "the tunnel is down and the system proxy has been left pointing at it, so traffic fails instead of leaving in the clear. Disconnect to put it back.".into(),
        );
    }
}

pub fn handle_exit(app: &AppContext, inner: &Arc<SupervisorInner>, generation: u64, reason: String) {
    let holding = {
        let session = lock(&inner.session);
        session.as_ref().is_some_and(|current| current.profile.kill_switch)
            && proxy_is_applied(inner)
    };
    let decision = decide_exit(&mut lock(&inner.session), generation, holding);
    let (attempt, base_profile) = match decision {
        ExitDecision::Ignore => return,
        ExitDecision::GiveUp { profile } => return give_up(app, inner, generation, &profile, reason),
        ExitDecision::Hold { profile } => return hold(app, inner, generation, profile, reason),
        ExitDecision::Retry { attempt, profile } => (attempt, profile),
    };

    let profile = profile_for_attempt(&base_profile, attempt);
    let delay = retry_delay(attempt);
    let fell_back = fell_back_to_discovery(&base_profile, &profile);
    {
        let mut snapshot = lock(&inner.snapshot);
        if !inner.is_current(generation) {
            return;
        }
        snapshot.state = "reconnecting".into();
        snapshot.pid = None;
        snapshot.attempt = attempt;
        snapshot.status_message = Some(if fell_back {
            format!(
                "The pinned endpoint failed · searching for a working one · retry {attempt} of {MAX_ATTEMPTS} on {} in {}s",
                transport_name(&profile),
                delay.as_secs()
            )
        } else {
            format!(
                "{reason} · retry {attempt} of {MAX_ATTEMPTS} on {} in {}s",
                transport_name(&profile),
                delay.as_secs()
            )
        });
        mark_snapshot_dirty(inner);
    }
    if fell_back {
        supervisor_log(
            inner,
            "warn",
            format!(
                "custom endpoint {} failed; falling back to automatic discovery",
                base_profile.peer.as_deref().unwrap_or("(unset)")
            ),
        );
    }
    supervisor_log(
        inner,
        "warn",
        format!(
            "{reason}; retry {attempt} of {MAX_ATTEMPTS} on {} in {}s",
            transport_label(&profile),
            delay.as_secs()
        ),
    );

    let app = app.clone();
    let inner = inner.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + delay;
        while Instant::now() < deadline {
            if !inner.is_current(generation) {
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
        if !inner.is_current(generation) {
            return;
        }
        if let Err(error) = launch(&app, &inner, &profile, attempt, generation, true) {
            handle_exit(&app, &inner, generation, error);
        }
    });
}

pub fn hold(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    generation: u64,
    profile: CoreProfile,
    reason: String,
) {
    {
        let mut snapshot = lock(&inner.snapshot);
        if !inner.is_current(generation) {
            return;
        }
        snapshot.state = "error".into();
        snapshot.pid = None;
        snapshot.attempt = 0;
        snapshot.blocking = true;
        snapshot.status_message = Some(format!(
            "Traffic is blocked while the tunnel is down · trying again every {}s",
            HOLD_RETRY.as_secs()
        ));
        snapshot.last_error = Some(format!(
            "{reason}. Traffic is being held rather than sent in the clear — the search continues in the background, or disconnect to put your system proxy back."
        ));
        mark_snapshot_dirty(inner);
    }
    supervisor_log(
        inner,
        "warn",
        format!("{reason}; holding traffic and retrying every {}s", HOLD_RETRY.as_secs()),
    );

    let app = app.clone();
    let inner = inner.clone();
    thread::spawn(move || {
        let deadline = Instant::now() + HOLD_RETRY;
        while Instant::now() < deadline {
            if !inner.is_current(generation) {
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
        if !inner.is_current(generation) {
            return;
        }
        let next = profile_for_attempt(&profile, 1);
        if let Err(error) = launch(&app, &inner, &next, 1, generation, true) {
            handle_exit(&app, &inner, generation, error);
        }
    });
}

pub fn profile_for_attempt(base: &CoreProfile, attempt: u32) -> CoreProfile {
    let mut profile = base.clone();
    if attempt > 0 && base.protocol == "masque" && attempt % 2 == 1 {
        profile.masque_transport = if base.masque_transport == "h2" {
            "h3".into()
        } else {
            "h2".into()
        };
    }
    if attempt > 0 && base.endpoint_mode == "custom-first" {
        profile.endpoint_mode = "automatic".into();
    }
    profile
}

pub fn fell_back_to_discovery(base: &CoreProfile, attempted: &CoreProfile) -> bool {
    base.endpoint_mode == "custom-first" && attempted.endpoint_mode == "automatic"
}

pub fn retry_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(5);
    Duration::from_secs((BASE_RETRY_SECS << shift).min(MAX_RETRY_SECS))
}

pub fn transport_label(profile: &CoreProfile) -> &'static str {
    match (profile.protocol.as_str(), profile.masque_transport.as_str()) {
        ("masque", "h2") => "masque-h2",
        ("masque", _) => "masque-h3",
        ("wg", _) => "wireguard",
        _ => "warp-in-warp",
    }
}

pub fn transport_name(profile: &CoreProfile) -> &'static str {
    match transport_label(profile) {
        "masque-h2" => "MASQUE H2",
        "masque-h3" => "MASQUE H3",
        "wireguard" => "WireGuard",
        _ => "WARP in WARP",
    }
}

pub fn session_summary(profile: &CoreProfile, attempt: u32) -> String {
    format!(
        "session transport={} scan={} ip={} noize={} fragment={} dataCheck={} quickReconnect={} perf={} validate={}s startup={}s endpoint={} peerPinned={} zeroTrust={} gateway={} attempt={attempt}",
        transport_label(profile),
        profile.scan_mode,
        profile.ip_family,
        profile.noize,
        profile.fragment_client_hello,
        profile.data_check,
        profile.quick_reconnect,
        profile.performance_profile,
        profile.validate_secs,
        profile.startup_secs,
        profile.endpoint_mode,
        non_empty(profile.peer.as_deref()).is_some(),
        non_empty(profile.team.as_deref()).is_some(),
        profile.gateway,
    )
}
