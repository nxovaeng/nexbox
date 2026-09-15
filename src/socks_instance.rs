//! Multi-instance SOCKS5 listener and management for VPS multi-region egress.
//!
//! Features:
//! - Multiple SOCKS5 ports listening simultaneously (e.g., 1081, 1082, 1083...)
//! - SOCKS5 Username / Password authentication per instance (RFC 1928 / RFC 1929)
//! - HTTP CONNECT support with Basic Auth on the same port
//! - Individual lifecycle control: start, stop, restart each instance independently
//! - Auto-start on VPS boot / process startup
//! - Independent upstream binding (Warp, Proton, Windscribe, Psiphon, Tor, Custom)
//! - Real connectivity test (real exit IP, country, ASN/colo, RTT)
//! - Real speed test (measured throughput in Mbps)

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::app_context::AppContext;
use crate::http_bridge::socks5_connect_with_auth;

const CONFIG_FILENAME: &str = "socks_instances.json";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const PROBE_TIMEOUT: Duration = Duration::from_secs(12);
const SPEED_TIMEOUT: Duration = Duration::from_secs(30);
const SPEED_BYTES: usize = 5_000_000; // 5 MB test payload

// ── Models ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpstreamType {
    Warp,
    Proton,
    Windscribe,
    Psiphon,
    Tor,
    Custom,
}

impl Default for UpstreamType {
    fn default() -> Self {
        Self::Warp
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocksInstanceConfig {
    pub id: String,
    pub name: String,
    #[serde(default = "default_listen_host")]
    pub listen_host: String,
    pub listen_port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub autostart: bool,
    pub upstream_type: UpstreamType,
    #[serde(default)]
    pub upstream_config: serde_json::Value,
}

fn default_listen_host() -> String {
    "0.0.0.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityResult {
    pub success: bool,
    pub ip: Option<String>,
    pub country: Option<String>,
    pub colo: Option<String>,
    pub org: Option<String>,
    pub latency_ms: Option<f64>,
    pub error: Option<String>,
    pub checked_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedResult {
    pub success: bool,
    pub mbps: Option<f64>,
    pub bytes: usize,
    pub duration_secs: f64,
    pub error: Option<String>,
    pub tested_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocksInstanceStatus {
    pub id: String,
    pub is_running: bool,
    pub state: String, // "stopped" | "starting" | "running" | "error"
    pub bound_address: Option<String>,
    pub upstream_address: Option<String>,
    pub last_error: Option<String>,
    pub started_at: Option<u64>,
    pub last_connectivity: Option<ConnectivityResult>,
    pub last_speed: Option<SpeedResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceView {
    #[serde(flatten)]
    pub config: SocksInstanceConfig,
    pub status: SocksInstanceStatus,
}

// ── Active Instance Handle ───────────────────────────────────────────────────

struct ActiveInstance {
    stop_signal: Arc<AtomicBool>,
    bound_addr: SocketAddr,
    upstream_addr: SocketAddr,
    child_process: Option<Arc<std::sync::Mutex<Option<Child>>>>,
}

// ── Manager ──────────────────────────────────────────────────────────────────

pub struct SocksInstanceManager {
    configs: RwLock<Vec<SocksInstanceConfig>>,
    statuses: RwLock<HashMap<String, SocksInstanceStatus>>,
    actives: Arc<std::sync::Mutex<HashMap<String, ActiveInstance>>>,
    config_file: std::sync::Mutex<Option<PathBuf>>,
}

impl Default for SocksInstanceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SocksInstanceManager {
    pub fn new() -> Self {
        Self {
            configs: RwLock::new(Vec::new()),
            statuses: RwLock::new(HashMap::new()),
            actives: Arc::new(std::sync::Mutex::new(HashMap::new())),
            config_file: std::sync::Mutex::new(None),
        }
    }

    /// Initializes manager with storage path and loads configs.
    pub async fn init(&self, app: &AppContext) {
        let config_dir = app.app_config_dir().unwrap_or_else(|_| PathBuf::from("config"));
        let path = config_dir.join(CONFIG_FILENAME);
        *self.config_file.lock().unwrap() = Some(path.clone());

        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(loaded) = serde_json::from_str::<Vec<SocksInstanceConfig>>(&data) {
                    let mut cfg_lock = self.configs.write().await;
                    *cfg_lock = loaded;
                }
            }
        } else {
            // Seed initial sensible defaults for a multi-region VPS gateway
            let defaults = vec![
                SocksInstanceConfig {
                    id: "warp-gateway".to_string(),
                    name: "Cloudflare Warp 出口".to_string(),
                    listen_host: "0.0.0.0".to_string(),
                    listen_port: 1081,
                    username: None,
                    password: None,
                    autostart: false,
                    upstream_type: UpstreamType::Warp,
                    upstream_config: serde_json::json!({
                        "mode": "masque"
                    }),
                },
                SocksInstanceConfig {
                    id: "proton-jp".to_string(),
                    name: "Proton 日本东京".to_string(),
                    listen_host: "0.0.0.0".to_string(),
                    listen_port: 1082,
                    username: None,
                    password: None,
                    autostart: false,
                    upstream_type: UpstreamType::Proton,
                    upstream_config: serde_json::json!({
                        "country": "JP"
                    }),
                },
                SocksInstanceConfig {
                    id: "windscribe-hk".to_string(),
                    name: "Windscribe 香港出口".to_string(),
                    listen_host: "0.0.0.0".to_string(),
                    listen_port: 1083,
                    username: None,
                    password: None,
                    autostart: false,
                    upstream_type: UpstreamType::Windscribe,
                    upstream_config: serde_json::json!({
                        "country": "HK"
                    }),
                },
            ];
            let mut cfg_lock = self.configs.write().await;
            *cfg_lock = defaults;
            drop(cfg_lock);
            self.save_configs_internal().await;
        }

        // Initialize status map for all configs
        let configs = self.configs.read().await.clone();
        let mut stat_lock = self.statuses.write().await;
        for c in configs {
            stat_lock.entry(c.id.clone()).or_insert_with(|| SocksInstanceStatus {
                id: c.id,
                is_running: false,
                state: "stopped".to_string(),
                bound_address: None,
                upstream_address: None,
                last_error: None,
                started_at: None,
                last_connectivity: None,
                last_speed: None,
            });
        }
    }

    /// Triggers auto-start for all configs that have `autostart: true`.
    pub fn trigger_autostart(&self, app: AppContext) {
        let mgr = app.socks_mgr.clone();
        tokio::spawn(async move {
            let configs = mgr.configs.read().await.clone();
            for cfg in configs {
                if cfg.autostart {
                    println!("[socks_mgr] Auto-starting instance '{}' (port {})...", cfg.name, cfg.listen_port);
                    if let Err(e) = mgr.start_instance(&app, &cfg.id).await {
                        eprintln!("[socks_mgr] Failed to autostart instance '{}': {e}", cfg.name);
                    }
                }
            }
        });
    }

    async fn save_configs_internal(&self) {
        let path_opt = self.config_file.lock().unwrap().clone();
        if let Some(path) = path_opt {
            let configs = self.configs.read().await.clone();
            if let Ok(json_str) = serde_json::to_string_pretty(&configs) {
                let _ = fs::write(&path, json_str);
            }
        }
    }

    /// Returns list of all instances with configs and runtime statuses.
    pub async fn list_instances(&self) -> Vec<InstanceView> {
        let configs = self.configs.read().await.clone();
        let statuses = self.statuses.read().await.clone();
        let actives = self.actives.lock().unwrap();

        let mut out = Vec::new();
        for c in configs {
            let mut st = statuses.get(&c.id).cloned().unwrap_or_else(|| SocksInstanceStatus {
                id: c.id.clone(),
                is_running: false,
                state: "stopped".to_string(),
                bound_address: None,
                upstream_address: None,
                last_error: None,
                started_at: None,
                last_connectivity: None,
                last_speed: None,
            });

            if let Some(act) = actives.get(&c.id) {
                st.is_running = true;
                st.state = "running".to_string();
                st.bound_address = Some(act.bound_addr.to_string());
                st.upstream_address = Some(act.upstream_addr.to_string());
            }

            out.push(InstanceView {
                config: c,
                status: st,
            });
        }
        out
    }

    /// Add a new SOCKS5 instance.
    pub async fn create_instance(&self, cfg: SocksInstanceConfig) -> Result<InstanceView, String> {
        let mut configs = self.configs.write().await;
        // Check ID uniqueness
        if configs.iter().any(|c| c.id == cfg.id) {
            return Err(format!("Instance ID '{}' already exists", cfg.id));
        }
        // Check port uniqueness
        if configs.iter().any(|c| c.listen_port == cfg.listen_port) {
            return Err(format!("Port {} is already used by another instance", cfg.listen_port));
        }

        configs.push(cfg.clone());
        drop(configs);
        self.save_configs_internal().await;

        let st = SocksInstanceStatus {
            id: cfg.id.clone(),
            is_running: false,
            state: "stopped".to_string(),
            bound_address: None,
            upstream_address: None,
            last_error: None,
            started_at: None,
            last_connectivity: None,
            last_speed: None,
        };
        self.statuses.write().await.insert(cfg.id.clone(), st.clone());

        Ok(InstanceView {
            config: cfg,
            status: st,
        })
    }

    /// Update an existing SOCKS5 instance config.
    pub async fn update_instance(&self, cfg: SocksInstanceConfig) -> Result<InstanceView, String> {
        let mut configs = self.configs.write().await;
        let index = configs.iter().position(|c| c.id == cfg.id).ok_or_else(|| "Instance not found".to_string())?;

        // Check port uniqueness with others
        if configs.iter().enumerate().any(|(i, c)| i != index && c.listen_port == cfg.listen_port) {
            return Err(format!("Port {} is already used by another instance", cfg.listen_port));
        }

        configs[index] = cfg.clone();
        drop(configs);
        self.save_configs_internal().await;

        let statuses = self.statuses.read().await;
        let st = statuses.get(&cfg.id).cloned().unwrap_or_else(|| SocksInstanceStatus {
            id: cfg.id.clone(),
            is_running: false,
            state: "stopped".to_string(),
            bound_address: None,
            upstream_address: None,
            last_error: None,
            started_at: None,
            last_connectivity: None,
            last_speed: None,
        });

        Ok(InstanceView {
            config: cfg,
            status: st,
        })
    }

    /// Toggle autostart for an instance.
    pub async fn set_autostart(&self, id: &str, autostart: bool) -> Result<(), String> {
        let mut configs = self.configs.write().await;
        let c = configs.iter_mut().find(|c| c.id == id).ok_or_else(|| "Instance not found".to_string())?;
        c.autostart = autostart;
        drop(configs);
        self.save_configs_internal().await;
        Ok(())
    }

    /// Delete an instance (stops it first if running).
    pub async fn delete_instance(&self, id: &str) -> Result<(), String> {
        self.stop_instance(id).await?;
        let mut configs = self.configs.write().await;
        configs.retain(|c| c.id != id);
        drop(configs);
        self.save_configs_internal().await;
        self.statuses.write().await.remove(id);
        Ok(())
    }

    /// Starts a specific SOCKS5 instance and its associated upstream program.
    pub async fn start_instance(&self, app: &AppContext, id: &str) -> Result<SocksInstanceStatus, String> {
        // Stop any running instance with same ID first
        let _ = self.stop_instance(id).await;

        let config = {
            let configs = self.configs.read().await;
            configs.iter().find(|c| c.id == id).cloned().ok_or_else(|| "Instance not found".to_string())?
        };

        // Update status to starting
        {
            let mut statuses = self.statuses.write().await;
            if let Some(st) = statuses.get_mut(id) {
                st.state = "starting".to_string();
                st.last_error = None;
            }
        }

        // 1. Resolve or spawn upstream
        let (upstream_addr, child_proc) = match self.prepare_upstream(app, &config).await {
            Ok(res) => res,
            Err(e) => {
                let mut statuses = self.statuses.write().await;
                if let Some(st) = statuses.get_mut(id) {
                    st.state = "error".to_string();
                    st.last_error = Some(e.clone());
                }
                return Err(e);
            }
        };

        // 2. Bind the SOCKS5 listening port on VPS (0.0.0.0:listen_port)
        let bind_host: IpAddr = config.listen_host.parse().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let bind_sock = SocketAddr::new(bind_host, config.listen_port);

        let listener = TcpListener::bind(bind_sock).map_err(|e| {
            let msg = format!("Failed to bind port {}: {e}", config.listen_port);
            let mut actives = self.actives.lock().unwrap();
            actives.remove(id);
            msg
        })?;

        let actual_addr = listener.local_addr().map_err(|e| e.to_string())?;
        let stop_signal = Arc::new(AtomicBool::new(false));

        // Credentials for auth
        let auth_credentials = match (&config.username, &config.password) {
            (Some(u), Some(p)) if !u.trim().is_empty() && !p.trim().is_empty() => {
                Some((u.trim().to_string(), p.trim().to_string()))
            }
            _ => None,
        };

        // Spawn listener loop in background thread
        let loop_stop = stop_signal.clone();
        let loop_upstream = upstream_addr;
        let loop_auth = auth_credentials;
        let inst_name = config.name.clone();

        thread::Builder::new()
            .name(format!("socks-inst-{}", config.listen_port))
            .spawn(move || {
                for stream in listener.incoming() {
                    if loop_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(client) = stream else { continue };
                    let to = loop_upstream;
                    let auth = loop_auth.clone();
                    thread::spawn(move || {
                        let _ = handle_client_conn(client, to, auth.as_ref());
                    });
                }
                println!("[socks_mgr] Instance '{}' listener thread terminated", inst_name);
            })
            .map_err(|e| format!("Failed to spawn worker thread: {e}"))?;

        let now_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let child_shared = child_proc.map(|cp| Arc::new(std::sync::Mutex::new(Some(cp))));

        let active = ActiveInstance {
            stop_signal,
            bound_addr: actual_addr,
            upstream_addr,
            child_process: child_shared,
        };

        self.actives.lock().unwrap().insert(id.to_string(), active);

        let status = SocksInstanceStatus {
            id: id.to_string(),
            is_running: true,
            state: "running".to_string(),
            bound_address: Some(actual_addr.to_string()),
            upstream_address: Some(upstream_addr.to_string()),
            last_error: None,
            started_at: Some(now_ts),
            last_connectivity: None,
            last_speed: None,
        };

        self.statuses.write().await.insert(id.to_string(), status.clone());
        println!("[socks_mgr] Instance '{}' started successfully on {}", config.name, actual_addr);

        Ok(status)
    }

    /// Stops an instance and frees its port and child process.
    pub async fn stop_instance(&self, id: &str) -> Result<SocksInstanceStatus, String> {
        let removed = self.actives.lock().unwrap().remove(id);
        if let Some(act) = removed {
            act.stop_signal.store(true, Ordering::SeqCst);
            // Connect to unblock accept()
            let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, act.bound_addr.port())), Duration::from_millis(50));

            // Terminate child process if any
            if let Some(child_arc) = act.child_process {
                let mut guard = child_arc.lock().unwrap();
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }

        let mut statuses = self.statuses.write().await;
        let st = statuses.entry(id.to_string()).or_insert_with(|| SocksInstanceStatus {
            id: id.to_string(),
            is_running: false,
            state: "stopped".to_string(),
            bound_address: None,
            upstream_address: None,
            last_error: None,
            started_at: None,
            last_connectivity: None,
            last_speed: None,
        });

        st.is_running = false;
        st.state = "stopped".to_string();
        st.bound_address = None;
        st.upstream_address = None;

        Ok(st.clone())
    }

    /// Restart an instance.
    pub async fn restart_instance(&self, app: &AppContext, id: &str) -> Result<SocksInstanceStatus, String> {
        self.stop_instance(id).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        self.start_instance(app, id).await
    }

    /// Stops all running instances (for graceful daemon shutdown).
    pub fn shutdown_all(&self) {
        let mut actives = self.actives.lock().unwrap();
        for (id, act) in actives.drain() {
            println!("[socks_mgr] Stopping instance '{}'...", id);
            act.stop_signal.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, act.bound_addr.port())), Duration::from_millis(50));
            if let Some(child_arc) = act.child_process {
                let mut guard = child_arc.lock().unwrap();
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }

    /// Helper to prepare or launch upstream proxy for a given configuration.
    async fn prepare_upstream(&self, app: &AppContext, cfg: &SocksInstanceConfig) -> Result<(SocketAddr, Option<Child>), String> {
        match cfg.upstream_type {
            UpstreamType::Warp => {
                // If Aether core is already connected, forward to it
                if let Some(socks_str) = app.supervisor().connected_socks() {
                    if let Ok(addr) = socks_str.parse::<SocketAddr>() {
                        return Ok((addr, None));
                    }
                }
                // If mihomo chain is running, forward to it
                if let Some(addr) = app.chain().address() {
                    return Ok((addr, None));
                }
                // Fallback default Aether mixed port if listening
                let default_socks = SocketAddr::from((Ipv4Addr::LOCALHOST, 1820));
                if TcpStream::connect_timeout(&default_socks, Duration::from_millis(500)).is_ok() {
                    return Ok((default_socks, None));
                }

                // If not running, attempt starting core with active profile
                let profile = crate::core_supervisor::load_profile(app.clone()).await.unwrap_or_else(|_| crate::core_supervisor::CoreProfile::default());
                match crate::core_supervisor::start_core(app.clone(), app.supervisor(), profile).await {
                    Ok(snap) => {
                        if let Ok(addr) = snap.socks_address.parse::<SocketAddr>() {
                            return Ok((addr, None));
                        }
                    }
                    Err(e) => {
                        return Err(format!("Could not start Warp engine: {e}"));
                    }
                }
                Ok((default_socks, None))
            }
            UpstreamType::Proton => {
                // Check if Proton standalone is already active
                if app.proton().is_running() {
                    let info = app.proton().get_info(app).await;
                    if let Some(addr_str) = info.active_address {
                        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                            return Ok((addr, None));
                        }
                    }
                }

                // Parse country from upstream_config
                let country = cfg.upstream_config.get("country").and_then(|v| v.as_str()).map(|s| s.to_string());
                let server_name = cfg.upstream_config.get("serverName").and_then(|v| v.as_str()).map(|s| s.to_string());

                let mut settings = app.proton().load_settings(app);
                if country.is_some() {
                    settings.country = country;
                }
                if server_name.is_some() {
                    settings.server_name = server_name;
                }
                settings.listen_port = Some(find_free_port().unwrap_or(21008));

                match app.proton().start_standalone(app, &settings).await {
                    Ok(addr) => Ok((addr, None)),
                    Err(e) => Err(format!("Proton failed to start: {e}")),
                }
            }
            UpstreamType::Windscribe => {
                // Check if Windscribe standalone is already running
                if app.windscribe().is_running().await {
                    let snap = app.windscribe().snapshot().await;
                    if let Some(addr_str) = snap.active_address {
                        if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                            return Ok((addr, None));
                        }
                    }
                }

                // Start Windscribe standalone with selected country or server
                let mut settings = crate::windscribe::load_standalone_settings(app);
                if let Some(cc) = cfg.upstream_config.get("country").and_then(|v| v.as_str()) {
                    settings.country = Some(cc.to_string());
                }
                if let Some(tag) = cfg.upstream_config.get("serverTag").and_then(|v| v.as_str()) {
                    settings.server_tag = Some(tag.to_string());
                }
                settings.listen_port = Some(find_free_port().unwrap_or(21009));

                match app.windscribe().start(app, &settings).await {
                    Ok(addr) => Ok((addr, None)),
                    Err(e) => Err(format!("Windscribe failed to start: {e}")),
                }
            }
            UpstreamType::Psiphon => {
                let snap = app.psiphon().snapshot();
                if snap.state == "connected" {
                    if let Some(port) = snap.socks_port {
                        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                        return Ok((addr, None));
                    }
                }
                let mut settings = crate::psiphon::load_standalone_settings(app);
                if let Some(r) = cfg.upstream_config.get("region").and_then(|v| v.as_str()) {
                    settings.egress_region = r.to_string();
                }
                let port = find_free_port().unwrap_or(21010);
                settings.listen_port = Some(port);

                let app_c = app.clone();
                let psiphon = app.psiphon();
                let res = tokio::task::spawn_blocking(move || psiphon.start_standalone(&app_c, &settings)).await
                    .map_err(|e| format!("Task error: {e}"))?;
                match res {
                    Ok(addr) => Ok((addr, None)),
                    Err(e) => Err(format!("Psiphon failed to start: {e}")),
                }
            }
            UpstreamType::Tor => {
                let snap = app.tor().snapshot();
                if snap.state == "connected" {
                    if let Some(port) = snap.socks_port {
                        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
                        return Ok((addr, None));
                    }
                }
                let settings = crate::tor::TorSettings::default();
                let app_c = app.clone();
                let tor = app.tor();
                let res = tokio::task::spawn_blocking(move || tor.start(&app_c, &settings, None)).await
                    .map_err(|e| format!("Task error: {e}"))?;
                match res {
                    Ok(addr) => Ok((addr, None)),
                    Err(e) => Err(format!("Tor failed to start: {e}")),
                }
            }
            UpstreamType::Custom => {
                let target = cfg.upstream_config.get("address")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Custom upstream requires 'address' (e.g. 127.0.0.1:1080)".to_string())?;
                let addr: SocketAddr = target.parse().map_err(|e| format!("Invalid upstream address '{target}': {e}"))?;
                Ok((addr, None))
            }
        }
    }

    /// Performs real connectivity test through the SOCKS5 instance.
    pub async fn test_connectivity(&self, id: &str) -> Result<ConnectivityResult, String> {
        let (bound_port, auth) = {
            let configs = self.configs.read().await;
            let cfg = configs.iter().find(|c| c.id == id).ok_or_else(|| "Instance not found".to_string())?;
            let creds = match (&cfg.username, &cfg.password) {
                (Some(u), Some(p)) if !u.trim().is_empty() && !p.trim().is_empty() => Some((u.clone(), p.clone())),
                _ => None,
            };
            (cfg.listen_port, creds)
        };

        let active = {
            let actives = self.actives.lock().unwrap();
            actives.get(id).is_some()
        };
        if !active {
            return Err("Cannot test connectivity: SOCKS5 instance is not running".to_string());
        }

        let socks_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, bound_port));

        let res = tokio::task::spawn_blocking(move || {
            let creds_ref = auth.as_ref().map(|(u, p)| (u.as_str(), p.as_str()));
            probe_socks_connectivity(socks_addr, creds_ref)
        })
        .await
        .map_err(|e| format!("Task error: {e}"))?;

        // Update status with test result
        let mut statuses = self.statuses.write().await;
        if let Some(st) = statuses.get_mut(id) {
            st.last_connectivity = Some(res.clone());
        }

        Ok(res)
    }

    /// Performs real speed test through the SOCKS5 instance.
    pub async fn test_speed(&self, id: &str) -> Result<SpeedResult, String> {
        let (bound_port, auth) = {
            let configs = self.configs.read().await;
            let cfg = configs.iter().find(|c| c.id == id).ok_or_else(|| "Instance not found".to_string())?;
            let creds = match (&cfg.username, &cfg.password) {
                (Some(u), Some(p)) if !u.trim().is_empty() && !p.trim().is_empty() => Some((u.clone(), p.clone())),
                _ => None,
            };
            (cfg.listen_port, creds)
        };

        let active = {
            let actives = self.actives.lock().unwrap();
            actives.get(id).is_some()
        };
        if !active {
            return Err("Cannot test speed: SOCKS5 instance is not running".to_string());
        }

        let socks_addr = SocketAddr::from((Ipv4Addr::LOCALHOST, bound_port));

        let res = tokio::task::spawn_blocking(move || {
            let creds_ref = auth.as_ref().map(|(u, p)| (u.as_str(), p.as_str()));
            probe_socks_speed(socks_addr, creds_ref)
        })
        .await
        .map_err(|e| format!("Task error: {e}"))?;

        let mut statuses = self.statuses.write().await;
        if let Some(st) = statuses.get_mut(id) {
            st.last_speed = Some(res.clone());
        }

        Ok(res)
    }
}

// ── SOCKS5 & HTTP Server Engine ──────────────────────────────────────────────

const SOCKS_NO_AUTH: u8 = 0x00;
const SOCKS_USER_PASS: u8 = 0x02;
const SOCKS_NO_ACCEPTABLE: u8 = 0xFF;

fn handle_client_conn(mut client: TcpStream, upstream: SocketAddr, auth: Option<&(String, String)>) -> io::Result<()> {
    client.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;

    let mut first = [0_u8; 1];
    if client.read(&mut first)? == 0 {
        return Ok(());
    }

    if first[0] == 0x05 {
        handle_socks5(client, upstream, auth)
    } else {
        handle_http_connect(client, upstream, auth, first[0])
    }
}

fn handle_socks5(mut client: TcpStream, upstream: SocketAddr, auth: Option<&(String, String)>) -> io::Result<()> {
    let mut count = [0_u8; 1];
    client.read_exact(&mut count)?;
    let mut methods = vec![0_u8; count[0] as usize];
    client.read_exact(&mut methods)?;

    let wanted = if auth.is_some() { SOCKS_USER_PASS } else { SOCKS_NO_AUTH };
    if !methods.contains(&wanted) {
        client.write_all(&[0x05, SOCKS_NO_ACCEPTABLE])?;
        return Ok(());
    }
    client.write_all(&[0x05, wanted])?;

    if let Some((user, password)) = auth {
        if !verify_socks5_subneg(&mut client, user, password)? {
            return Ok(());
        }
    }

    // Read request: VER, CMD, RSV, ATYP
    let mut head = [0_u8; 4];
    client.read_exact(&mut head)?;
    if head[1] != 0x01 {
        // Only CONNECT supported
        return socks5_reply(&mut client, 0x07);
    }

    let host = match head[3] {
        0x01 => {
            let mut raw = [0_u8; 4];
            client.read_exact(&mut raw)?;
            Ipv4Addr::from(raw).to_string()
        }
        0x03 => {
            let mut len = [0_u8; 1];
            client.read_exact(&mut len)?;
            let mut name = vec![0_u8; len[0] as usize];
            client.read_exact(&mut name)?;
            String::from_utf8_lossy(&name).into_owned()
        }
        0x04 => {
            let mut raw = [0_u8; 16];
            client.read_exact(&mut raw)?;
            std::net::Ipv6Addr::from(raw).to_string()
        }
        _ => return socks5_reply(&mut client, 0x08),
    };

    let mut port_bytes = [0_u8; 2];
    client.read_exact(&mut port_bytes)?;
    let port = u16::from_be_bytes(port_bytes);

    // Forward through upstream SOCKS5 listener
    let upstream_stream = match socks5_connect_with_auth(upstream, &host, port, HANDSHAKE_TIMEOUT, None) {
        Ok(s) => s,
        Err(_) => return socks5_reply(&mut client, 0x05),
    };

    // Respond success: 0x00
    socks5_reply(&mut client, 0x00)?;
    client.set_read_timeout(None)?;

    // Splicing
    splice_connections(client, upstream_stream);
    Ok(())
}

fn verify_socks5_subneg(stream: &mut TcpStream, expected_user: &str, expected_pass: &str) -> io::Result<bool> {
    let mut ver = [0_u8; 1];
    stream.read_exact(&mut ver)?;
    if ver[0] != 0x01 {
        stream.write_all(&[0x01, 0x01])?;
        return Ok(false);
    }

    let mut ulen = [0_u8; 1];
    stream.read_exact(&mut ulen)?;
    let mut ubytes = vec![0_u8; ulen[0] as usize];
    stream.read_exact(&mut ubytes)?;
    let user = String::from_utf8_lossy(&ubytes);

    let mut plen = [0_u8; 1];
    stream.read_exact(&mut plen)?;
    let mut pbytes = vec![0_u8; plen[0] as usize];
    stream.read_exact(&mut pbytes)?;
    let pass = String::from_utf8_lossy(&pbytes);

    if user == expected_user && pass == expected_pass {
        stream.write_all(&[0x01, 0x00])?;
        Ok(true)
    } else {
        stream.write_all(&[0x01, 0x01])?;
        Ok(false)
    }
}

fn socks5_reply(stream: &mut TcpStream, rep: u8) -> io::Result<()> {
    stream.write_all(&[0x05, rep, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
}

fn handle_http_connect(mut client: TcpStream, upstream: SocketAddr, auth: Option<&(String, String)>, first_byte: u8) -> io::Result<()> {
    let mut reader = BufReader::new(client.try_clone()?);
    let mut line = String::new();
    line.push(first_byte as char);
    reader.read_line(&mut line)?;

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "CONNECT" {
        let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n");
        return Ok(());
    }
    let target = parts[1];

    // Read headers and check Proxy-Authorization if auth required
    let mut authenticated = auth.is_none();
    loop {
        let mut header_line = String::new();
        if reader.read_line(&mut header_line)? == 0 || header_line == "\r\n" || header_line == "\n" {
            break;
        }
        if let Some((user, pass)) = auth {
            let lower = header_line.to_ascii_lowercase();
            if lower.starts_with("proxy-authorization: basic ") {
                let encoded = header_line["proxy-authorization: basic ".len()..].trim();
                if let Ok(decoded) = BASE64_STANDARD.decode(encoded) {
                    if let Ok(creds) = String::from_utf8(decoded) {
                        if creds == format!("{user}:{pass}") {
                            authenticated = true;
                        }
                    }
                }
            }
        }
    }

    if !authenticated {
        let resp = "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"NextVPN VPS Gateway\"\r\nConnection: close\r\n\r\n";
        let _ = client.write_all(resp.as_bytes());
        return Ok(());
    }

    let (host, port) = match target.split_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(443)),
        None => (target, 443),
    };

    let upstream_stream = match socks5_connect_with_auth(upstream, host, port, HANDSHAKE_TIMEOUT, None) {
        Ok(s) => s,
        Err(_) => {
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n");
            return Ok(());
        }
    };

    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
    client.set_read_timeout(None)?;
    splice_connections(client, upstream_stream);
    Ok(())
}

fn splice_connections(client: TcpStream, upstream: TcpStream) {
    let Ok(client_r) = client.try_clone() else { return };
    let Ok(upstream_r) = upstream.try_clone() else { return };

    let outbound = thread::spawn(move || {
        let mut from = client_r;
        let mut to = upstream;
        let _ = io::copy(&mut from, &mut to);
        let _ = to.shutdown(Shutdown::Write);
    });

    let mut from = upstream_r;
    let mut to = client;
    let _ = io::copy(&mut from, &mut to);
    let _ = to.shutdown(Shutdown::Write);
    let _ = outbound.join();
}

// ── Connectivity & Speed Probers ─────────────────────────────────────────────

fn probe_socks_connectivity(socks: SocketAddr, auth: Option<(&str, &str)>) -> ConnectivityResult {
    let now_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let started = Instant::now();

    // 1. Connect through SOCKS5 to Cloudflare trace
    let mut stream = match socks5_connect_with_auth(socks, "www.cloudflare.com", 80, PROBE_TIMEOUT, auth) {
        Ok(s) => s,
        Err(e) => {
            return ConnectivityResult {
                success: false,
                ip: None,
                country: None,
                colo: None,
                org: None,
                latency_ms: None,
                error: Some(format!("Handshake failed: {e}")),
                checked_at: now_ts,
            };
        }
    };

    let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
    let req = "GET /cdn-cgi/trace HTTP/1.1\r\nHost: www.cloudflare.com\r\nUser-Agent: NextVPN-VPS/1.0\r\nConnection: close\r\n\r\n";
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return ConnectivityResult {
            success: false,
            ip: None,
            country: None,
            colo: None,
            org: None,
            latency_ms: None,
            error: Some(format!("Request write error: {e}")),
            checked_at: now_ts,
        };
    }

    let mut body = String::new();
    if let Err(e) = stream.read_to_string(&mut body) {
        return ConnectivityResult {
            success: false,
            ip: None,
            country: None,
            colo: None,
            org: None,
            latency_ms: None,
            error: Some(format!("Read response error: {e}")),
            checked_at: now_ts,
        };
    }

    let rtt = started.elapsed().as_secs_f64() * 1000.0;

    let mut ip = None;
    let mut country = None;
    let mut colo = None;

    for line in body.lines() {
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "ip" => ip = Some(v.trim().to_string()),
                "loc" => country = Some(v.trim().to_uppercase()),
                "colo" => colo = Some(v.trim().to_uppercase()),
                _ => {}
            }
        }
    }

    ConnectivityResult {
        success: ip.is_some(),
        ip,
        country,
        colo,
        org: Some("Cloudflare Edge Network".to_string()),
        latency_ms: Some((rtt * 10.0).round() / 10.0),
        error: None,
        checked_at: now_ts,
    }
}

fn probe_socks_speed(socks: SocketAddr, auth: Option<(&str, &str)>) -> SpeedResult {
    let now_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let mut stream = match socks5_connect_with_auth(socks, "speed.cloudflare.com", 80, SPEED_TIMEOUT, auth) {
        Ok(s) => s,
        Err(e) => {
            return SpeedResult {
                success: false,
                mbps: None,
                bytes: 0,
                duration_secs: 0.0,
                error: Some(format!("Connection refused: {e}")),
                tested_at: now_ts,
            };
        }
    };

    let _ = stream.set_read_timeout(Some(SPEED_TIMEOUT));
    let req = format!(
        "GET /__down?bytes={SPEED_BYTES} HTTP/1.1\r\nHost: speed.cloudflare.com\r\nUser-Agent: NextVPN-VPS/1.0\r\nConnection: close\r\n\r\n"
    );
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return SpeedResult {
            success: false,
            mbps: None,
            bytes: 0,
            duration_secs: 0.0,
            error: Some(format!("Request write error: {e}")),
            tested_at: now_ts,
        };
    }

    let mut buffer = [0_u8; 64 * 1024];
    let mut total_read = 0usize;
    let mut started: Option<Instant> = None;

    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                started.get_or_insert_with(Instant::now);
                total_read += n;
            }
            Err(e) => {
                return SpeedResult {
                    success: false,
                    mbps: None,
                    bytes: total_read,
                    duration_secs: 0.0,
                    error: Some(format!("Transfer interrupted: {e}")),
                    tested_at: now_ts,
                };
            }
        }
    }

    let duration = started.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.001).max(0.001);
    let mbps = (total_read as f64 * 8.0) / duration / 1_000_000.0;

    SpeedResult {
        success: total_read > 100_000,
        mbps: Some((mbps * 10.0).round() / 10.0),
        bytes: total_read,
        duration_secs: (duration * 100.0).round() / 100.0,
        error: None,
        tested_at: now_ts,
    }
}

fn find_free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok().map(|a| a.port()))
}
