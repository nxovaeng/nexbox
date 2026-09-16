use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
    time::Duration,
};
use serde::{Deserialize, Serialize};
use crate::carrier::CarrierChain;
use crate::chain::ChainSettings;
use crate::lan_share::LanSettings;

pub const MAX_LOGS: usize = 1_000;
pub const MAX_ATTEMPTS: u32 = 8;
pub const BASE_RETRY_SECS: u64 = 3;
pub const MAX_RETRY_SECS: u64 = 60;
pub const MAX_REPORT_BYTES: usize = 1_048_576;
pub const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CoreProfile {
    pub name: String,
    #[serde(default)]
    pub carriers: CarrierChain,
    pub protocol: String,
    pub masque_transport: String,
    pub scan_mode: String,
    pub ip_family: String,
    pub socks_address: String,
    pub quick_reconnect: bool,
    pub validate_secs: u64,
    pub startup_secs: u64,
    pub reconnect_secs: u64,
    pub dns: Vec<String>,
    pub fragment_client_hello: bool,
    pub fragment_size: String,
    pub fragment_delay: String,
    pub data_check: bool,
    pub h2_peer: Option<String>,
    pub ech: Option<String>,
    pub tls_groups: Option<String>,
    pub performance_profile: String,
    pub keepalive_secs: u16,
    pub noize: String,
    pub profile_retry: bool,
    pub log_level: String,
    pub endpoint_mode: String,
    pub peer: Option<String>,
    pub wg_peer: Option<String>,
    pub core_path: Option<String>,
    pub route_block: String,
    pub route_direct: String,
    pub routes_file: Option<String>,
    #[serde(default)]
    pub full_tunnel: bool,
    #[serde(default)]
    pub upstream_proxy: String,
    #[serde(default = "yes")]
    pub route_sniff: bool,
    #[serde(default = "yes")]
    pub auto_reprovision: bool,
    #[serde(default, alias = "bypass_iran_sites")]
    pub bypass_direct_routes: bool,
    pub team: Option<String>,
    pub access_client_id: Option<String>,
    pub access_client_secret: Option<String>,
    pub access_email: Option<String>,
    pub access_token: Option<String>,
    pub gateway: bool,
    pub system_proxy: bool,
    pub auto_reconnect: bool,
    pub chain: ChainSettings,
    #[serde(default)]
    pub psiphon: crate::psiphon::PsiphonSettings,
    #[serde(default)]
    pub tor: crate::tor::TorSettings,
    #[serde(default)]
    pub lan_share: LanSettings,
    pub kill_switch: bool,
}

pub fn yes() -> bool {
    true
}

impl Default for CoreProfile {
    fn default() -> Self {
        Self {
            name: "Adaptive".into(),
            carriers: CarrierChain::default(),
            protocol: "masque".into(),
            masque_transport: "h2".into(),
            scan_mode: "balanced".into(),
            ip_family: "both".into(),
            socks_address: "127.0.0.1:1819".into(),
            quick_reconnect: true,
            validate_secs: 10,
            startup_secs: 30,
            reconnect_secs: 2,
            dns: vec!["1.1.1.1".into(), "1.0.0.1".into()],
            fragment_client_hello: true,
            fragment_size: "16-32".into(),
            fragment_delay: "2-10".into(),
            data_check: true,
            h2_peer: None,
            ech: None,
            tls_groups: None,
            performance_profile: "auto".into(),
            keepalive_secs: 25,
            auto_reconnect: true,
            chain: ChainSettings::default(),
            psiphon: crate::psiphon::PsiphonSettings::default(),
            tor: crate::tor::TorSettings::default(),
            lan_share: LanSettings::default(),
            kill_switch: false,
            noize: "balanced".into(),
            profile_retry: true,
            log_level: "info".into(),
            endpoint_mode: "automatic".into(),
            peer: None,
            wg_peer: None,
            core_path: None,
            route_block: String::new(),
            route_direct: String::new(),
            routes_file: None,
            bypass_direct_routes: false,
            full_tunnel: false,
            upstream_proxy: String::new(),
            route_sniff: true,
            auto_reprovision: true,
            team: None,
            access_client_id: None,
            access_client_secret: None,
            access_email: None,
            access_token: None,
            gateway: false,
            system_proxy: false,
        }
    }
}

impl CoreProfile {
    pub fn validate(&self) -> Result<(), String> {
        require_one_of("protocol", &self.protocol, &["masque", "wg", "gool"])?;
        require_one_of("MASQUE transport", &self.masque_transport, &["h2", "h3"])?;
        require_one_of(
            "scan mode",
            &self.scan_mode,
            &["turbo", "balanced", "thorough", "stealth", "ironclad"],
        )?;
        require_one_of("IP family", &self.ip_family, &["v4", "v6", "both"])?;
        require_one_of(
            "log level",
            &self.log_level,
            &["error", "warn", "info", "debug", "trace"],
        )?;
        require_one_of(
            "Noize profile",
            &self.noize,
            &["off", "light", "firewall", "balanced", "gfw", "aggressive"],
        )?;
        require_one_of(
            "performance profile",
            &self.performance_profile,
            &["auto", "low", "medium", "high"],
        )?;
        require_one_of(
            "endpoint mode",
            &self.endpoint_mode,
            &["automatic", "custom-first", "custom-only"],
        )?;
        if self.endpoint_mode != "automatic" && non_empty(self.peer.as_deref()).is_none() {
            return Err("pinning an endpoint requires a custom address".into());
        }

        self.socks_address
            .parse::<SocketAddr>()
            .map_err(|_| "SOCKS address must be a valid IP:port".to_string())?;
        if !(1..=120).contains(&self.validate_secs) {
            return Err("validation deadline must be between 1 and 120 seconds".into());
        }
        if !(5..=300).contains(&self.startup_secs) {
            return Err("startup deadline must be between 5 and 300 seconds".into());
        }
        if self.reconnect_secs > 120 {
            return Err("reconnect delay must not exceed 120 seconds".into());
        }
        if !(0..=300).contains(&self.keepalive_secs) {
            return Err("WireGuard keepalive must be between 1 and 300 seconds".into());
        }
        if self.dns.is_empty() || self.dns.len() > 8 {
            return Err("one to eight DNS resolvers are required".into());
        }
        for resolver in &self.dns {
            resolver
                .parse::<IpAddr>()
                .map_err(|_| format!("invalid DNS resolver: {resolver}"))?;
        }
        validate_range("fragment size", &self.fragment_size, 1, 1_500)?;
        validate_range("fragment delay", &self.fragment_delay, 0, 10_000)?;
        validate_peer("peer", self.peer.as_deref())?;
        validate_peer("WireGuard peer", self.wg_peer.as_deref())?;
        validate_peer("HTTP/2 peer", self.h2_peer.as_deref())?;
        validate_optional_text("ECH configuration", self.ech.as_deref(), 32_768)?;
        validate_optional_text("TLS groups", self.tls_groups.as_deref(), 4_096)?;
        validate_text("block rules", &self.route_block, 64_000)?;
        validate_text("direct rules", &self.route_direct, 64_000)?;
        validate_optional_text("routes file", self.routes_file.as_deref(), 4_096)?;
        validate_optional_text("team", self.team.as_deref(), 253)?;
        validate_optional_text("Access client ID", self.access_client_id.as_deref(), 4_096)?;
        validate_optional_text(
            "Access client secret",
            self.access_client_secret.as_deref(),
            4_096,
        )?;
        validate_optional_text("Access email", self.access_email.as_deref(), 320)?;
        validate_optional_text("Access token", self.access_token.as_deref(), 32_768)?;
        Ok(())
    }

    pub fn process_log_level(&self) -> &str {
        match self.log_level.as_str() {
            "debug" | "trace" => self.log_level.as_str(),
            _ => "info",
        }
    }

    pub fn args(&self, identity_path: &Path) -> Vec<String> {
        let mut args = vec![
            format!("--{}", self.protocol),
            "--scan".into(),
            self.scan_mode.clone(),
            "--ip".into(),
            self.ip_family.clone(),
            "--bind".into(),
            self.socks_address.clone(),
            "--validate-secs".into(),
            self.validate_secs.to_string(),
            "--startup-secs".into(),
            self.startup_secs.to_string(),
            "--reconnect-secs".into(),
            self.reconnect_secs.to_string(),
            "--dns".into(),
            self.dns.join(","),
            "--noize".into(),
            self.noize.clone(),
            "--log-level".into(),
            self.process_log_level().into(),
            "--config".into(),
            identity_path.to_string_lossy().into_owned(),
        ];

        if self.keepalive_secs > 0 {
            args.extend(["--keepalive".into(), self.keepalive_secs.to_string()]);
        }
        if self.protocol == "masque" && self.masque_transport == "h2" {
            args.push("--h2".into());
        }
        if !self.data_check {
            args.push("--no-data-check".into());
        }
        args.push(if self.quick_reconnect {
            "--quick-reconnect".into()
        } else {
            "--no-quick-reconnect".into()
        });
        if self.fragment_client_hello && self.protocol == "masque" && self.masque_transport == "h2" {
            args.extend([
                "--fragment".into(),
                "--fragment-size".into(),
                self.fragment_size.clone(),
                "--fragment-delay".into(),
                self.fragment_delay.clone(),
            ]);
        }
        if !self.profile_retry {
            args.push("--no-profile-retry".into());
        }
        if self.endpoint_mode != "automatic" {
            if let Some(peer) = non_empty(self.peer.as_deref()) {
                args.extend(["--peer".into(), peer.into()]);
            }
        }
        if let Some(peer) = non_empty(self.wg_peer.as_deref()) {
            args.extend(["--wg-peer".into(), peer.into()]);
        }
        if let Some(peer) = non_empty(self.h2_peer.as_deref()) {
            args.extend(["--h2-peer".into(), peer.into()]);
        }
        if let Some(ech) = non_empty(self.ech.as_deref()).filter(|value| *value != "off") {
            args.extend(["--ech".into(), ech.into()]);
        }
        if let Some(groups) = non_empty(self.tls_groups.as_deref()) {
            args.extend(["--tls-groups".into(), groups.into()]);
        }
        if self.performance_profile != "auto" {
            args.extend(["--perf".into(), self.performance_profile.clone()]);
        }
        if !self.route_block.trim().is_empty() {
            args.extend(["--route-block".into(), self.route_block.clone()]);
        }
        if !self.route_direct.trim().is_empty() {
            args.extend(["--route-direct".into(), self.route_direct.clone()]);
        }
        if let Some(path) = non_empty(self.routes_file.as_deref()) {
            args.extend(["--routes".into(), path.into()]);
        }
        if self.gateway {
            args.push("--gateway".into());
        }
        args
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSnapshot {
    pub state: String,
    pub pid: Option<u32>,
    pub core_path: Option<String>,
    pub version: Option<String>,
    pub transport: Option<String>,
    pub endpoint: Option<String>,
    pub socks_address: String,
    pub latency_ms: Option<f64>,
    pub started_at: Option<u64>,
    pub last_error: Option<String>,
    pub status_message: Option<String>,
    pub attempt: u32,
    pub max_attempts: u32,
    pub blocking: bool,
}

impl Default for CoreSnapshot {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            pid: None,
            core_path: None,
            version: None,
            transport: None,
            endpoint: None,
            socks_address: "127.0.0.1:1819".into(),
            latency_ms: None,
            started_at: None,
            last_error: None,
            status_message: None,
            attempt: 0,
            max_attempts: MAX_ATTEMPTS,
            blocking: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreLogEvent {
    pub timestamp: u64,
    pub stream: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreProbe {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub protocol: Option<String>,
    pub masque_transport: Option<String>,
    pub endpoint_mode: Option<String>,
    pub peer: Option<String>,
    pub dns: Option<Vec<String>>,
    pub socks_address: Option<String>,
    pub upstream_proxy: Option<String>,
    pub noize: Option<String>,
    pub fragment_client_hello: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreItem {
    pub name: String,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyRoute {
    Tunnel(SocketAddr),
    Chain(SocketAddr),
}

pub struct Session {
    pub generation: u64,
    pub profile: CoreProfile,
    pub attempt: u32,
}

#[derive(Debug)]
pub enum ExitDecision {
    Retry { attempt: u32, profile: CoreProfile },
    GiveUp { profile: CoreProfile },
    Hold { profile: CoreProfile },
    Ignore,
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

pub fn require_one_of(label: &str, value: &str, options: &[&str]) -> Result<(), String> {
    if options.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "invalid {label}: {value} (expected one of {})",
            options.join(", ")
        ))
    }
}

pub fn validate_peer(label: &str, value: Option<&str>) -> Result<(), String> {
    let Some(value) = non_empty(value) else {
        return Ok(());
    };
    value
        .parse::<SocketAddr>()
        .map(|_| ())
        .map_err(|_| format!("{label} must be a valid IP:port (e.g. 162.159.192.1:2408)"))
}

pub fn validate_range(label: &str, value: &str, minimum: u64, maximum: u64) -> Result<(), String> {
    let parts: Vec<&str> = value.split('-').collect();
    let valid = match parts.as_slice() {
        [single] => single.parse::<u64>().map(|n| (n, n)).ok(),
        [first, second] => {
            let a = first.parse::<u64>().ok();
            let b = second.parse::<u64>().ok();
            match (a, b) {
                (Some(a), Some(b)) if a <= b => Some((a, b)),
                _ => None,
            }
        }
        _ => None,
    };
    match valid {
        Some((low, high)) if low >= minimum && high <= maximum => Ok(()),
        _ => Err(format!(
            "{label} must be a number or range between {minimum} and {maximum} (got \"{value}\")"
        )),
    }
}

pub fn validate_optional_text(label: &str, value: Option<&str>, maximum: usize) -> Result<(), String> {
    if let Some(text) = non_empty(value) {
        validate_text(label, text, maximum)?;
    }
    Ok(())
}

pub fn validate_text(label: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.len() > maximum {
        return Err(format!("{label} is too long (maximum {maximum} bytes)"));
    }
    Ok(())
}

pub fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
