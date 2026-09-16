use std::path::PathBuf;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::app_context::AppContext;
use crate::core_supervisor::CoreProfile;

const CONFIG_FILENAME: &str = "aether_profiles.json";

fn default_protocol() -> String { "masque".to_string() }
fn default_masque_transport() -> String { "h2".to_string() }
fn default_scan_mode() -> String { "balanced".to_string() }
fn default_ip_family() -> String { "both".to_string() }
fn default_socks_address() -> String { "127.0.0.1:1819".to_string() }
fn default_noize() -> String { "off".to_string() }
fn default_fragment_size() -> String { "16-32".to_string() }
fn default_fragment_delay() -> String { "2-10".to_string() }
fn default_endpoint_mode() -> String { "automatic".to_string() }
fn default_dns() -> Vec<String> { vec!["1.1.1.1".into(), "1.0.0.1".into()] }
fn default_keepalive() -> u16 { 25 }
fn default_true() -> bool { true }
fn default_log_level() -> String { "info".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AetherProfileConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_protocol")]
    pub protocol: String, // "masque" | "wg" | "gool"
    #[serde(default = "default_masque_transport")]
    pub masque_transport: String, // "h2" | "h3"
    #[serde(default = "default_scan_mode")]
    pub scan_mode: String, // "turbo" | "balanced" | "thorough" | "stealth" | "ironclad"
    #[serde(default = "default_ip_family")]
    pub ip_family: String, // "both" | "v4" | "v6"
    #[serde(default = "default_socks_address")]
    pub socks_address: String,
    #[serde(default = "default_noize")]
    pub noize: String, // "off" | "light" | "balanced" | "firewall" | "gfw" | "aggressive"
    #[serde(default)]
    pub fragment_client_hello: bool,
    #[serde(default = "default_fragment_size")]
    pub fragment_size: String,
    #[serde(default = "default_fragment_delay")]
    pub fragment_delay: String,
    #[serde(default = "default_endpoint_mode")]
    pub endpoint_mode: String, // "automatic" | "custom-first" | "custom-only"
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default = "default_dns")]
    pub dns: Vec<String>,
    #[serde(default = "default_keepalive")]
    pub keepalive_secs: u16,
    #[serde(default = "default_true")]
    pub quick_reconnect: bool,
    #[serde(default = "default_true")]
    pub data_check: bool,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default)]
    pub is_active: bool,
}

impl Default for AetherProfileConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "海外 VPS 极速直连".to_string(),
            description: Some("关闭混流与分片，最小化协议开销，延迟最低、吞吐最大".to_string()),
            protocol: "masque".to_string(),
            masque_transport: "h3".to_string(),
            scan_mode: "turbo".to_string(),
            ip_family: "both".to_string(),
            socks_address: "127.0.0.1:1819".to_string(),
            noize: "off".to_string(),
            fragment_client_hello: false,
            fragment_size: "16-32".to_string(),
            fragment_delay: "2-10".to_string(),
            endpoint_mode: "automatic".to_string(),
            peer: None,
            dns: vec!["1.1.1.1".into(), "1.0.0.1".into()],
            keepalive_secs: 25,
            quick_reconnect: true,
            data_check: true,
            log_level: "info".to_string(),
            is_active: true,
        }
    }
}

impl AetherProfileConfig {
    /// Converts this profile configuration to a CoreProfile suitable for core_supervisor.
    pub fn to_core_profile(&self) -> CoreProfile {
        let mut p = CoreProfile::default();
        p.name = self.name.clone();
        p.protocol = self.protocol.clone();
        p.masque_transport = self.masque_transport.clone();
        p.scan_mode = self.scan_mode.clone();
        p.ip_family = self.ip_family.clone();
        p.socks_address = self.socks_address.clone();
        p.noize = self.noize.clone();
        p.fragment_client_hello = self.fragment_client_hello;
        p.fragment_size = self.fragment_size.clone();
        p.fragment_delay = self.fragment_delay.clone();
        p.endpoint_mode = self.endpoint_mode.clone();
        p.peer = self.peer.clone();
        p.dns = self.dns.clone();
        p.keepalive_secs = self.keepalive_secs;
        p.quick_reconnect = self.quick_reconnect;
        p.data_check = self.data_check;
        p.log_level = self.log_level.clone();
        // Carriers is always lone aether for single-protocol Aether outbound:
        p.carriers = crate::carrier::CarrierChain::default();
        p.chain.enabled = false;
        p
    }

    /// Converts an existing CoreProfile into an AetherProfileConfig.
    pub fn from_core_profile(id: &str, p: &CoreProfile, is_active: bool) -> Self {
        Self {
            id: id.to_string(),
            name: if p.name.trim().is_empty() { id.to_string() } else { p.name.clone() },
            description: None,
            protocol: p.protocol.clone(),
            masque_transport: p.masque_transport.clone(),
            scan_mode: p.scan_mode.clone(),
            ip_family: p.ip_family.clone(),
            socks_address: p.socks_address.clone(),
            noize: p.noize.clone(),
            fragment_client_hello: p.fragment_client_hello,
            fragment_size: p.fragment_size.clone(),
            fragment_delay: p.fragment_delay.clone(),
            endpoint_mode: p.endpoint_mode.clone(),
            peer: p.peer.clone(),
            dns: p.dns.clone(),
            keepalive_secs: p.keepalive_secs,
            quick_reconnect: p.quick_reconnect,
            data_check: p.data_check,
            log_level: p.log_level.clone(),
            is_active,
        }
    }
}

pub struct AetherProfileManager {
    configs: RwLock<Vec<AetherProfileConfig>>,
    config_file: Mutex<Option<PathBuf>>,
}

impl Default for AetherProfileManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AetherProfileManager {
    pub fn new() -> Self {
        Self {
            configs: RwLock::new(Vec::new()),
            config_file: Mutex::new(None),
        }
    }

    /// Default sensible profiles for multi-scenario VPS deployments.
    fn default_presets() -> Vec<AetherProfileConfig> {
        vec![
            AetherProfileConfig {
                id: "default".to_string(),
                name: "海外 VPS 极速直连".to_string(),
                description: Some("关闭混流与分片，QUIC 传输，适合无封锁海外服务器直连，低开销低延迟".to_string()),
                protocol: "masque".to_string(),
                masque_transport: "h3".to_string(),
                scan_mode: "turbo".to_string(),
                ip_family: "both".to_string(),
                socks_address: "127.0.0.1:1819".to_string(),
                noize: "off".to_string(),
                fragment_client_hello: false,
                fragment_size: "16-32".to_string(),
                fragment_delay: "2-10".to_string(),
                endpoint_mode: "automatic".to_string(),
                peer: None,
                dns: vec!["1.1.1.1".into(), "1.0.0.1".into()],
                keepalive_secs: 25,
                quick_reconnect: true,
                data_check: true,
                log_level: "info".to_string(),
                is_active: true,
            },
            AetherProfileConfig {
                id: "stealth-h2".to_string(),
                name: "受管控 / 抗封锁 H2 混流".to_string(),
                description: Some("开启 TLS ClientHello 分片 (16-32B, 2-10ms)、HTTP/2 传输与 Firewall 混淆流，穿透受控网络".to_string()),
                protocol: "masque".to_string(),
                masque_transport: "h2".to_string(),
                scan_mode: "balanced".to_string(),
                ip_family: "both".to_string(),
                socks_address: "127.0.0.1:1820".to_string(),
                noize: "firewall".to_string(),
                fragment_client_hello: true,
                fragment_size: "16-32".to_string(),
                fragment_delay: "2-10".to_string(),
                endpoint_mode: "automatic".to_string(),
                peer: None,
                dns: vec!["1.1.1.1".into(), "1.0.0.1".into()],
                keepalive_secs: 25,
                quick_reconnect: true,
                data_check: true,
                log_level: "info".to_string(),
                is_active: false,
            },
            AetherProfileConfig {
                id: "gfw-aggressive".to_string(),
                name: "GFW 深度穿透 (高强度对抗)".to_string(),
                description: Some("极细粒度 TLS 分片 (8-16B, 5-15ms) 与 GFW 深度混淆流，对抗严苛审查".to_string()),
                protocol: "masque".to_string(),
                masque_transport: "h2".to_string(),
                scan_mode: "stealth".to_string(),
                ip_family: "both".to_string(),
                socks_address: "127.0.0.1:1821".to_string(),
                noize: "gfw".to_string(),
                fragment_client_hello: true,
                fragment_size: "8-16".to_string(),
                fragment_delay: "5-15".to_string(),
                endpoint_mode: "automatic".to_string(),
                peer: None,
                dns: vec!["1.1.1.1".into(), "1.0.0.1".into()],
                keepalive_secs: 25,
                quick_reconnect: true,
                data_check: true,
                log_level: "info".to_string(),
                is_active: false,
            },
            AetherProfileConfig {
                id: "native-wireguard".to_string(),
                name: "WireGuard 原生直连".to_string(),
                description: Some("经典 WireGuard UDP 隧道协议，直连 Cloudflare Warp 端点".to_string()),
                protocol: "wg".to_string(),
                masque_transport: "h2".to_string(),
                scan_mode: "balanced".to_string(),
                ip_family: "both".to_string(),
                socks_address: "127.0.0.1:1822".to_string(),
                noize: "off".to_string(),
                fragment_client_hello: false,
                fragment_size: "16-32".to_string(),
                fragment_delay: "2-10".to_string(),
                endpoint_mode: "automatic".to_string(),
                peer: None,
                dns: vec!["1.1.1.1".into(), "1.0.0.1".into()],
                keepalive_secs: 25,
                quick_reconnect: true,
                data_check: true,
                log_level: "info".to_string(),
                is_active: false,
            },
        ]
    }

    /// Initializes manager and loads `config/aether_profiles.json`.
    pub async fn init(&self, app: &AppContext) {
        let config_file = match app.app_config_dir() {
            Ok(p) => p.join(CONFIG_FILENAME),
            Err(_) => PathBuf::from("config").join(CONFIG_FILENAME),
        };

        {
            let mut guard = self.config_file.lock().unwrap();
            *guard = Some(config_file.clone());
        }

        if config_file.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&config_file).await {
                if let Ok(parsed) = serde_json::from_str::<Vec<AetherProfileConfig>>(&content) {
                    if !parsed.is_empty() {
                        let mut lock = self.configs.write().await;
                        *lock = parsed;
                        println!("[aether_profiles] Loaded {} profiles from {}", lock.len(), config_file.display());
                        return;
                    }
                }
            }
        }

        // Initialize default presets
        let presets = Self::default_presets();
        let mut lock = self.configs.write().await;
        *lock = presets;
        drop(lock);
        self.save_internal().await;
        println!("[aether_profiles] Initialized default presets to {}", config_file.display());
    }

    async fn save_internal(&self) {
        let file_path = {
            let guard = self.config_file.lock().unwrap();
            guard.clone()
        };

        if let Some(path) = file_path {
            let configs = self.configs.read().await.clone();
            if let Ok(json) = serde_json::to_string_pretty(&configs) {
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                let _ = tokio::fs::write(&path, json).await;
            }
        }
    }

    /// List all Aether profiles.
    pub async fn list(&self) -> Vec<AetherProfileConfig> {
        self.configs.read().await.clone()
    }

    /// Get a profile by ID.
    pub async fn get(&self, id: &str) -> Option<AetherProfileConfig> {
        let configs = self.configs.read().await;
        configs.iter().find(|c| c.id == id).cloned()
    }

    /// Get the active profile, or fallback to first.
    pub async fn get_active(&self) -> AetherProfileConfig {
        let configs = self.configs.read().await;
        configs.iter().find(|c| c.is_active).cloned()
            .or_else(|| configs.first().cloned())
            .unwrap_or_default()
    }

    /// Set a profile as the active one.
    pub async fn set_active(&self, id: &str) -> Result<AetherProfileConfig, String> {
        let mut configs = self.configs.write().await;
        let mut target = None;
        for c in configs.iter_mut() {
            if c.id == id {
                c.is_active = true;
                target = Some(c.clone());
            } else {
                c.is_active = false;
            }
        }
        let target = target.ok_or_else(|| format!("Profile '{id}' not found"))?;
        drop(configs);
        self.save_internal().await;
        Ok(target)
    }

    /// Save (create or update) a profile.
    pub async fn save(&self, profile: AetherProfileConfig) -> Result<AetherProfileConfig, String> {
        let mut configs = self.configs.write().await;
        if let Some(idx) = configs.iter().position(|c| c.id == profile.id) {
            configs[idx] = profile.clone();
        } else {
            configs.push(profile.clone());
        }
        drop(configs);
        self.save_internal().await;
        Ok(profile)
    }

    /// Duplicate an existing profile with a new name.
    pub async fn duplicate(&self, source_id: &str, new_name: &str) -> Result<AetherProfileConfig, String> {
        let mut configs = self.configs.write().await;
        let source = configs.iter().find(|c| c.id == source_id).cloned()
            .ok_or_else(|| format!("Source profile '{source_id}' not found"))?;

        let display_name = if new_name.trim().is_empty() {
            format!("{} (Copy)", source.name)
        } else {
            new_name.trim().to_string()
        };

        let raw_id: String = display_name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let trimmed = raw_id.trim_matches('-');
        let candidate_id = if trimmed.is_empty() {
            format!("copy-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
        } else {
            trimmed.to_string()
        };

        let final_id = if configs.iter().any(|c| c.id == candidate_id) {
            format!("{}-{}", candidate_id, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() % 10000)
        } else {
            candidate_id
        };

        let mut dup = source;
        dup.id = final_id;
        dup.name = display_name;
        dup.is_active = false;
        configs.push(dup.clone());
        drop(configs);
        self.save_internal().await;
        Ok(dup)
    }

    /// Delete a profile by ID.
    pub async fn delete(&self, id: &str) -> Result<(), String> {
        let mut configs = self.configs.write().await;
        if configs.len() <= 1 {
            return Err("Cannot delete the only remaining profile".to_string());
        }
        let pos = configs.iter().position(|c| c.id == id)
            .ok_or_else(|| format!("Profile '{id}' not found"))?;

        let was_active = configs[pos].is_active;
        configs.remove(pos);
        if was_active && !configs.is_empty() {
            configs[0].is_active = true;
        }
        drop(configs);
        self.save_internal().await;
        Ok(())
    }
}
