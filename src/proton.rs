use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::prelude::*;
use ring::digest::{digest, SHA512};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::app_context::AppContext;

pub const PROTON_DIR_NAME: &str = "proton";
pub const CONFIG_FILENAME: &str = "config.json";
pub const SESSION_FILENAME: &str = "session.json";
pub const SERVERS_CACHE_FILENAME: &str = "servers.json";
pub const WIRE_PROTON_CONF_FILENAME: &str = "wire_proton.conf";

pub const DEFAULT_STANDALONE_PORT: u16 = 10810;
pub const DEFAULT_STANDALONE_ADDRESS: &str = "127.0.0.1";

pub const SPOOFED_APP_VERSION: &str = "5.18.75.1";
pub const SPOOFED_MODEL: &str = "Pixel 9";
pub const SPOOFED_MANUFACTURER: &str = "Google";
pub const SPOOFED_ANDROID_VERSION: &str = "14";
pub const SPOOFED_USER_AGENT: &str = "PVPN/5.18.75.1 (Android 14; Google Pixel 9)";
pub const SPOOFED_APP_VERSION_HEADER: &str = "android-vpn@5.18.75.1-dev+play";

pub const PROTON_API_BASE_URL: &str = "https://vpn-api.proton.me";

// ── Android Device & Challenge Simulation ──────────────────────────────────────

/// Calculates a 32-bit Java String.hashCode() value, matching Kotlin/Android runtime.
pub fn java_string_hashcode(s: &str) -> i32 {
    let mut h: i32 = 0;
    for b in s.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as i32);
    }
    h
}

/// Generates a deterministic device hash based on machine-id / hostname.
pub fn get_device_hash() -> i32 {
    let machine_id = fs::read_to_string("/etc/machine-id")
        .or_else(|_| fs::read_to_string("/var/lib/dbus/machine-id"))
        .unwrap_or_else(|_| {
            std::env::var("HOSTNAME")
                .or_else(|_| std::env::var("COMPUTERNAME"))
                .unwrap_or_else(|_| "nextvpn-pixel-9".to_string())
        });
    java_string_hashcode(machine_id.trim())
}

/// Builds the `vpn-android-v4-challenge-0` challenge JSON payload required by Proton API.
pub fn build_challenge_payload(device_hash: i32) -> serde_json::Value {
    serde_json::json!({
        "Payload": {
            "vpn-android-v4-challenge-0": {
                "type": "me.proton.core.challenge.data.frame.ChallengeFrame.Device",
                "v": SPOOFED_APP_VERSION,
                "appLang": "en",
                "timezone": "America/New_York",
                "deviceName": device_hash,
                "regionCode": "US",
                "timezoneOffset": 240,
                "isJailbreak": false,
                "preferredContentSize": "1.0",
                "storageCapacity": 128.0,
                "isDarkmodeOn": false,
                "keyboards": ["com.google.android.inputmethod.latin"]
            }
        }
    })
}

// ── Cryptography & Key Derivation ─────────────────────────────────────────────

/// Generates a WireGuard-compatible private key and Ed25519 SubjectPublicKeyInfo PEM.
///
/// 1. 32-byte CSPRNG seed.
/// 2. WireGuard private key: SHA-512(seed), clamped (h[0] &= 248, h[31] &= 127, h[31] |= 64), first 32 bytes base64.
/// 3. Ed25519 public key derived from seed.
/// 4. PEM SubjectPublicKeyInfo (DER prefix 302a300506032b6570032100 + 32 bytes pubkey).
pub fn generate_vpn_keys() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("RNG failed: {e}"))?;

    // WireGuard private key from Ed25519 seed
    let h512 = digest(&SHA512, &seed);
    let mut wg_bytes = [0u8; 32];
    wg_bytes.copy_from_slice(&h512.as_ref()[..32]);
    wg_bytes[0] &= 248;
    wg_bytes[31] &= 127;
    wg_bytes[31] |= 64;
    let wg_private_key_b64 = BASE64_STANDARD.encode(&wg_bytes);

    // Ed25519 public key
    let keypair = Ed25519KeyPair::from_seed_unchecked(&seed)
        .map_err(|e| format!("Ed25519 key derivation failed: {e}"))?;
    let pubkey_bytes = keypair.public_key().as_ref();

    // DER SubjectPublicKeyInfo prefix for Ed25519 (RFC 8410)
    let der_prefix = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];
    let mut spki = Vec::with_capacity(12 + 32);
    spki.extend_from_slice(&der_prefix);
    spki.extend_from_slice(pubkey_bytes);

    let spki_b64 = BASE64_STANDARD.encode(&spki);
    let pem = format!("-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n", spki_b64);

    Ok((wg_private_key_b64, pem))
}

// ── Models & State ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ProtonSettings {
    /// Local SOCKS5 listening address (e.g. "127.0.0.1" or "0.0.0.0").
    pub listen_address: Option<String>,
    /// Local SOCKS5 listening port (default: 10810).
    pub listen_port: Option<u16>,
    /// Preferred exit country code (e.g. "US", "NL", "JP", "CH" or empty for auto).
    pub country: Option<String>,
    /// Specific preferred server name (e.g. "US-FREE#1").
    pub server_name: Option<String>,
    /// Max tier allowed (0 for Free, 1 for Basic, 2 for Plus).
    pub tier: u32,
    /// Auto failover to lowest-load server in the same country if current drops.
    pub auto_failover: bool,
}

impl Default for ProtonSettings {
    fn default() -> Self {
        Self {
            listen_address: Some(DEFAULT_STANDALONE_ADDRESS.to_string()),
            listen_port: Some(DEFAULT_STANDALONE_PORT),
            country: Some("US".to_string()),
            server_name: None,
            tier: 0,
            auto_failover: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ProtonSession {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub uid: String,
    pub user_id: Option<String>,
    pub max_tier: u32,
    pub wg_private_key: Option<String>,
    pub wg_certificate: Option<String>,
    pub cert_expires_at: Option<i64>,
    pub auth_mode: String, // "guest" | "imported"
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonServer {
    pub id: String,
    pub name: String,
    pub country: String,
    pub city: Option<String>,
    pub tier: u32,
    pub score: f64,
    pub load: u32,
    pub entry_ip: String,
    pub x25519_public_key: String,
    pub endpoint_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProtonCountrySummary {
    pub code: String,
    pub count: usize,
    pub lowest_load: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonServerSummary {
    pub name: String,
    pub country: String,
    pub city: Option<String>,
    pub load: u32,
    pub tier: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtonInfoResponse {
    pub wireproxy_installed: bool,
    pub wireproxy_path: Option<String>,
    pub is_running: bool,
    pub active_address: Option<String>,
    pub active_port: Option<u16>,
    pub active_server: Option<String>,
    pub active_country: Option<String>,
    pub session_active: bool,
    pub auth_mode: String,
    pub user_tier: u32,
    pub cert_expires_at: Option<i64>,
    pub cert_days_remaining: Option<f64>,
    pub total_servers: usize,
    pub countries: Vec<ProtonCountrySummary>,
    pub servers: Vec<ProtonServerSummary>,
    pub settings: ProtonSettings,
}

// ── Proton Manager ────────────────────────────────────────────────────────────

pub struct Proton {
    process: Mutex<Option<Child>>,
    is_running: AtomicBool,
    active_address: Mutex<Option<String>>,
    active_port: Mutex<Option<u16>>,
    active_server: Mutex<Option<String>>,
    active_country: Mutex<Option<String>>,
    servers_cache: Arc<RwLock<Vec<ProtonServer>>>,
    servers_last_fetched: Arc<RwLock<u64>>,
    watchdog_running: AtomicBool,
}

impl Proton {
    pub fn new() -> Self {
        Self {
            process: Mutex::new(None),
            is_running: AtomicBool::new(false),
            active_address: Mutex::new(None),
            active_port: Mutex::new(None),
            active_server: Mutex::new(None),
            active_country: Mutex::new(None),
            servers_cache: Arc::new(RwLock::new(Vec::new())),
            servers_last_fetched: Arc::new(RwLock::new(0)),
            watchdog_running: AtomicBool::new(false),
        }
    }

    /// Locates or creates the `data/proton` directory within NextVPN's app data dir.
    pub fn proton_dir(&self, app: &AppContext) -> PathBuf {
        let base = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("data"));
        let dir = base.join(PROTON_DIR_NAME);
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// Loads settings from `data/proton/config.json`.
    pub fn load_settings(&self, app: &AppContext) -> ProtonSettings {
        let path = self.proton_dir(app).join(CONFIG_FILENAME);
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<ProtonSettings>(&content) {
                return settings;
            }
        }
        ProtonSettings::default()
    }

    /// Saves settings to `data/proton/config.json` avoiding handle leaks with atomic write.
    pub fn save_settings(&self, app: &AppContext, settings: &ProtonSettings) -> Result<(), String> {
        let dir = self.proton_dir(app);
        let path = dir.join(CONFIG_FILENAME);
        let tmp_path = dir.join(format!("{CONFIG_FILENAME}.tmp.{}", std::process::id()));

        let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        fs::write(&tmp_path, json).map_err(|e| format!("Failed to write temporary settings: {e}"))?;
        fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to commit settings: {e}"))?;
        Ok(())
    }

    /// Loads active session from `data/proton/session.json`.
    /// Safely discards empty or corrupted session files (e.g., from sudden power outage or system crash).
    pub fn load_session(&self, app: &AppContext) -> Option<ProtonSession> {
        let path = self.proton_dir(app).join(SESSION_FILENAME);
        if !path.exists() {
            return None;
        }

        match fs::read_to_string(&path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    let _ = fs::remove_file(&path);
                    return None;
                }
                match serde_json::from_str::<ProtonSession>(&content) {
                    Ok(session) => {
                        if !session.access_token.is_empty() {
                            Some(session)
                        } else {
                            eprintln!("[proton] session.json has empty access_token, discarding");
                            let _ = fs::remove_file(&path);
                            None
                        }
                    }
                    Err(e) => {
                        eprintln!("[proton] session.json is corrupted ({e}), removing invalid file to allow auto-recovery");
                        let _ = fs::remove_file(&path);
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!("[proton] Failed to read {SESSION_FILENAME}: {e}");
                None
            }
        }
    }

    /// Ensures an active, valid Proton session is loaded and has a healthy certificate.
    /// Handles post-downtime scenarios, power loss, and token expirations:
    /// 1. If no session exists or was corrupted on disk -> automatically logs in as guest.
    /// 2. If certificate is missing or expiring soon (< 1h) -> attempts renewal.
    /// 3. If renewal fails due to 401 or invalid session on Proton servers -> re-logs in as guest.
    pub async fn ensure_valid_session(&self, app: &AppContext) -> Result<ProtonSession, String> {
        let mut session = match self.load_session(app) {
            Some(s) => s,
            None => {
                println!("[proton] No valid local session found (machine downtime or fresh startup). Performing automatic guest login...");
                return self.login_guest(app).await;
            }
        };

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let cert_exp = session.cert_expires_at.unwrap_or(0);
        let needs_cert_renew = cert_exp == 0 || (cert_exp - now) < 3600 || session.wg_private_key.is_none();

        if needs_cert_renew {
            println!(
                "[proton] Certificate missing or expiring soon (< 1h, exp: {cert_exp}, now: {now}). Attempting renewal..."
            );
            match self.register_7day_certificate(app, &mut session).await {
                Ok(()) => {
                    println!("[proton] Certificate successfully renewed with existing session");
                    Ok(session)
                }
                Err(e) => {
                    eprintln!(
                        "[proton] Certificate renewal with current session failed ({e}). Session likely expired during downtime. Re-logging in as guest..."
                    );
                    self.login_guest(app).await
                }
            }
        } else {
            Ok(session)
        }
    }

    /// Saves session to `data/proton/session.json` atomically.
    pub fn save_session(&self, app: &AppContext, session: &ProtonSession) -> Result<(), String> {
        let dir = self.proton_dir(app);
        let path = dir.join(SESSION_FILENAME);
        let tmp_path = dir.join(format!("{SESSION_FILENAME}.tmp.{}", std::process::id()));

        let json = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
        fs::write(&tmp_path, json).map_err(|e| format!("Failed to write temporary session: {e}"))?;
        fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to commit session: {e}"))?;
        Ok(())
    }

    /// Loads cached servers from `data/proton/servers.json` into memory.
    pub async fn load_cached_servers(&self, app: &AppContext) -> Vec<ProtonServer> {
        // Return memory cache if already populated
        {
            let cache = self.servers_cache.read().await;
            if !cache.is_empty() {
                return cache.clone();
            }
        }

        let path = self.proton_dir(app).join(SERVERS_CACHE_FILENAME);
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(servers) = serde_json::from_str::<Vec<ProtonServer>>(&content) {
                let mut cache = self.servers_cache.write().await;
                *cache = servers.clone();
                return servers;
            }
        }
        Vec::new()
    }

    /// Saves cached servers to `data/proton/servers.json` atomically and updates in-memory cache.
    pub async fn save_cached_servers(&self, app: &AppContext, servers: Vec<ProtonServer>) -> Result<(), String> {
        let dir = self.proton_dir(app);
        let path = dir.join(SERVERS_CACHE_FILENAME);
        let tmp_path = dir.join(format!("{SERVERS_CACHE_FILENAME}.tmp.{}", std::process::id()));

        let json = serde_json::to_string(&servers).map_err(|e| e.to_string())?;
        fs::write(&tmp_path, json).map_err(|e| format!("Failed to write temporary servers cache: {e}"))?;
        fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to commit servers cache: {e}"))?;

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        {
            let mut cache = self.servers_cache.write().await;
            *cache = servers;
        }
        {
            let mut last = self.servers_last_fetched.write().await;
            *last = now;
        }
        Ok(())
    }

    // ── Proton API Client Calls ───────────────────────────────────────────────

    /// Creates an HTTP client configured with spoofed Google Pixel 9 Android VPN headers.
    fn create_http_client(&self) -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(SPOOFED_USER_AGENT)
            .build()
            .map_err(|e| e.to_string())
    }

    /// Performs the full guest login flow via Proton API:
    /// Phase 0: Request anonymous session (`/auth/v4/sessions`)
    /// Phase 1: Upgrade to credential-less guest session (`/auth/v4/credentialless`)
    pub async fn login_guest(&self, app: &AppContext) -> Result<ProtonSession, String> {
        let client = self.create_http_client()?;
        let device_hash = get_device_hash();
        let payload = build_challenge_payload(device_hash);

        // Phase 0: Anonymous session
        let url_sess = format!("{PROTON_API_BASE_URL}/auth/v4/sessions");
        let resp_sess = client
            .post(&url_sess)
            .header("x-pm-appversion", SPOOFED_APP_VERSION_HEADER)
            .header("x-pm-apiversion", "4")
            .header("Accept", "application/vnd.protonmail.v1+json")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Phase 0 session network error: {e}"))?;

        let data_sess: serde_json::Value = resp_sess
            .json()
            .await
            .map_err(|e| format!("Phase 0 parse JSON error: {e}"))?;

        let code_sess = data_sess.get("Code").and_then(|v| v.as_i64()).unwrap_or(0);
        if code_sess != 1000 {
            return Err(format!(
                "Failed to create anonymous session (Code {code_sess}): {}",
                data_sess.get("Error").and_then(|v| v.as_str()).unwrap_or("Unknown")
            ));
        }

        let anon_token = data_sess
            .get("AccessToken")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "No AccessToken returned in Phase 0".to_string())?;
        let anon_uid = data_sess
            .get("UID")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "No UID returned in Phase 0".to_string())?;

        // Phase 1: Credential-less guest upgrade
        let url_cred = format!("{PROTON_API_BASE_URL}/auth/v4/credentialless");
        let resp_cred = client
            .post(&url_cred)
            .header("Authorization", format!("Bearer {anon_token}"))
            .header("x-pm-uid", anon_uid)
            .header("x-pm-appversion", SPOOFED_APP_VERSION_HEADER)
            .header("x-pm-apiversion", "4")
            .header("Accept", "application/vnd.protonmail.v1+json")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Phase 1 guest upgrade network error: {e}"))?;

        let data_cred: serde_json::Value = resp_cred
            .json()
            .await
            .map_err(|e| format!("Phase 1 parse JSON error: {e}"))?;

        let code_cred = data_cred.get("Code").and_then(|v| v.as_i64()).unwrap_or(0);
        if code_cred != 1000 {
            return Err(format!(
                "Failed guest credential-less login (Code {code_cred}): {}",
                data_cred.get("Error").and_then(|v| v.as_str()).unwrap_or("Unknown")
            ));
        }

        let access_token = data_cred
            .get("AccessToken")
            .and_then(|v| v.as_str())
            .unwrap_or(anon_token)
            .to_string();
        let uid = data_cred
            .get("UID")
            .and_then(|v| v.as_str())
            .unwrap_or(anon_uid)
            .to_string();
        let refresh_token = data_cred
            .get("RefreshToken")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let user_id = data_cred
            .get("UserID")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        let mut session = ProtonSession {
            access_token,
            refresh_token,
            uid,
            user_id,
            max_tier: 0,
            wg_private_key: None,
            wg_certificate: None,
            cert_expires_at: None,
            auth_mode: "guest".to_string(),
            updated_at: now,
        };

        // Automatically register 7-day certificate
        self.register_7day_certificate(app, &mut session).await?;
        self.save_session(app, &session)?;

        // Automatically refresh server list
        let _ = self.do_refresh_servers(app, &session).await;

        Ok(session)
    }

    /// Requests a WireGuard certificate from Proton API `/vpn/v1/certificate`.
    /// Sends `Duration: "10080 min"` and `DeviceName: "Google Pixel 9 (<device_hash>)"`
    /// to obtain a 7-day valid certificate as requested.
    pub async fn register_7day_certificate(
        &self,
        app: &AppContext,
        session: &mut ProtonSession,
    ) -> Result<(), String> {
        let (wg_priv_b64, pem) = generate_vpn_keys()?;
        let client = self.create_http_client()?;
        let device_hash = get_device_hash();
        let device_name = format!("{SPOOFED_MANUFACTURER} {SPOOFED_MODEL} ({device_hash})");

        let url = format!("{PROTON_API_BASE_URL}/vpn/v1/certificate");
        let payload = serde_json::json!({
            "ClientPublicKey": pem,
            "Mode": "session",
            "Duration": "10080 min",
            "DeviceName": device_name,
        });

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", session.access_token))
            .header("x-pm-uid", &session.uid)
            .header("User-Agent", SPOOFED_USER_AGENT)
            .header("x-pm-appversion", SPOOFED_APP_VERSION_HEADER)
            .header("x-pm-apiversion", "4")
            .header("Accept", "application/vnd.protonmail.v1+json")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Certificate request network error: {e}"))?;

        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Certificate parse JSON error: {e}"))?;

        let code = data.get("Code").and_then(|v| v.as_i64()).unwrap_or(0);
        if code != 1000 {
            return Err(format!(
                "Failed to register 7-day certificate (Code {code}): {}",
                data.get("Error").and_then(|v| v.as_str()).unwrap_or("Unknown")
            ));
        }

        let cert = data
            .get("Certificate")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let exp = data
            .get("ExpirationTime")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| {
                // Default to 7 days from now if not explicitly provided
                (SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() + 7 * 86400) as i64
            });

        session.wg_private_key = Some(wg_priv_b64);
        session.wg_certificate = Some(cert);
        session.cert_expires_at = Some(exp);
        session.updated_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        self.save_session(app, session)?;
        Ok(())
    }

    /// Fetches logical servers and load metrics from Proton API.
    /// Automatically detects expired sessions (e.g. after machine downtime),
    /// re-authenticates via guest login, and retries once.
    pub async fn refresh_servers(
        &self,
        app: &AppContext,
        session: &ProtonSession,
    ) -> Result<Vec<ProtonServer>, String> {
        match self.do_refresh_servers(app, session).await {
            Ok(servers) => Ok(servers),
            Err(e) => {
                if e.contains("401") || e.contains("expired") || e.contains("10013") {
                    println!("[proton] Session expired while refreshing servers ({e}). Re-logging in as guest...");
                    let new_session = self.login_guest(app).await?;
                    self.do_refresh_servers(app, &new_session).await
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn do_refresh_servers(
        &self,
        app: &AppContext,
        session: &ProtonSession,
    ) -> Result<Vec<ProtonServer>, String> {
        let client = self.create_http_client()?;

        // 1. Fetch servers
        let url_servers = format!("{PROTON_API_BASE_URL}/vpn/v2/logicals?WithEntriesForProtocols=wireguard&WithState=true");
        let resp_servers = client
            .get(&url_servers)
            .header("Authorization", format!("Bearer {}", session.access_token))
            .header("x-pm-uid", &session.uid)
            .header("User-Agent", SPOOFED_USER_AGENT)
            .header("x-pm-appversion", SPOOFED_APP_VERSION_HEADER)
            .header("x-pm-apiversion", "4")
            .header("Accept", "application/vnd.protonmail.v1+json")
            .send()
            .await
            .map_err(|e| format!("Fetch servers network error: {e}"))?;

        if resp_servers.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err("HTTP 401 Unauthorized: Proton session expired".to_string());
        }

        let data_servers: serde_json::Value = resp_servers
            .json()
            .await
            .map_err(|e| format!("Parse servers JSON error: {e}"))?;

        let code_servers = data_servers.get("Code").and_then(|v| v.as_i64()).unwrap_or(0);
        if code_servers == 401 || code_servers == 10013 {
            return Err(format!("Proton auth error (Code {code_servers}): session expired"));
        }

        let logicals = data_servers
            .get("LogicalServers")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                let err_msg = data_servers.get("Error").and_then(|v| v.as_str()).unwrap_or("No LogicalServers returned");
                format!("Failed to retrieve LogicalServers (Code {code_servers}): {err_msg}")
            })?;

        // 2. Fetch server loads
        let mut loads_map: HashMap<String, u32> = HashMap::new();
        let url_loads = format!("{PROTON_API_BASE_URL}/vpn/v1/loads");
        if let Ok(resp_loads) = client
            .get(&url_loads)
            .header("Authorization", format!("Bearer {}", session.access_token))
            .header("x-pm-uid", &session.uid)
            .header("User-Agent", SPOOFED_USER_AGENT)
            .header("x-pm-appversion", SPOOFED_APP_VERSION_HEADER)
            .header("x-pm-apiversion", "4")
            .header("Accept", "application/vnd.protonmail.v1+json")
            .send()
            .await
        {
            if let Ok(data_loads) = resp_loads.json::<serde_json::Value>().await {
                if let Some(arr) = data_loads.get("LogicalServers").and_then(|v| v.as_array()) {
                    for item in arr {
                        if let (Some(id), Some(load)) = (
                            item.get("ID").and_then(|v| v.as_str()),
                            item.get("Load").and_then(|v| v.as_u64()),
                        ) {
                            loads_map.insert(id.to_string(), load as u32);
                        }
                    }
                }
            }
        }

        // 3. Transform into ProtonServer models
        let mut servers: Vec<ProtonServer> = Vec::new();
        for item in logicals {
            let id = match item.get("ID").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let name = item.get("Name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let country = item.get("ExitCountry").and_then(|v| v.as_str()).unwrap_or("").to_uppercase();
            let city = item.get("City").and_then(|v| v.as_str()).map(|s| s.to_string());
            let tier = item.get("Tier").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let score = item.get("Score").and_then(|v| v.as_f64()).unwrap_or(99.0);
            let load = loads_map
                .get(&id)
                .cloned()
                .or_else(|| item.get("Load").and_then(|v| v.as_u64()).map(|v| v as u32))
                .unwrap_or(50);

            // Extract physical server entry
            let physical_servers = item.get("Servers").and_then(|v| v.as_array());
            let phys = match physical_servers.and_then(|arr| arr.first()) {
                Some(p) => p,
                None => continue,
            };

            let entry_ip = match phys.get("EntryIP").and_then(|v| v.as_str()) {
                Some(ip) => ip.to_string(),
                None => continue,
            };
            let x25519_public_key = match phys.get("X25519PublicKey").and_then(|v| v.as_str()) {
                Some(k) => k.to_string(),
                None => continue,
            };

            let mut endpoint_port: u16 = 51820;
            if let Some(wg_ports) = phys.get("WGPorts") {
                if let Some(udp_ports) = wg_ports.get("UDP").and_then(|v| v.as_array()) {
                    if let Some(first_port) = udp_ports.first().and_then(|p| p.as_u64()) {
                        endpoint_port = first_port as u16;
                    }
                }
            }

            servers.push(ProtonServer {
                id,
                name,
                country,
                city,
                tier,
                score,
                load,
                entry_ip,
                x25519_public_key,
                endpoint_port,
            });
        }

        self.save_cached_servers(app, servers.clone()).await?;
        Ok(servers)
    }

    // ── WireProxy Configuration & Launching ───────────────────────────────────

    /// Renders `wire_proton.conf` configuration content for WireProxy.
    pub fn render_wire_proton_conf(
        private_key: &str,
        server: &ProtonServer,
        listen_address: &str,
        listen_port: u16,
    ) -> String {
        format!(
            "[Interface]\n\
             PrivateKey = {}\n\
             Address = 10.2.0.2/32\n\
             DNS = 10.2.0.1\n\n\
             [Peer]\n\
             PublicKey = {}\n\
             Endpoint = {}:{}\n\
             PersistentKeepalive = 25\n\n\
             [Socks5]\n\
             BindAddress = {}:{}\n",
            private_key.trim(),
            server.x25519_public_key.trim(),
            server.entry_ip.trim(),
            server.endpoint_port,
            listen_address.trim(),
            listen_port
        )
    }

    /// Selects the best candidate server matching the given country and tier criteria.
    pub async fn select_server(
        &self,
        app: &AppContext,
        country: Option<&str>,
        server_name: Option<&str>,
        max_tier: u32,
        exclude_server_names: &[String],
    ) -> Option<ProtonServer> {
        let servers = self.load_cached_servers(app).await;
        if servers.is_empty() {
            return None;
        }

        // Direct name match (must match user's tier and requested country)
        if let Some(name) = server_name {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                if let Some(found) = servers.iter().find(|s| {
                    s.name.eq_ignore_ascii_case(trimmed)
                        && s.tier <= max_tier
                        && !exclude_server_names.contains(&s.name)
                        && (country.is_none() || country.unwrap().trim().is_empty() || s.country.eq_ignore_ascii_case(country.unwrap().trim()))
                }) {
                    return Some(found.clone());
                }
            }
        }

        // Filter candidates by tier, exclusions, and country
        let mut candidates: Vec<&ProtonServer> = servers
            .iter()
            .filter(|s| s.tier <= max_tier)
            .filter(|s| !exclude_server_names.contains(&s.name))
            .collect();

        if let Some(cc) = country {
            let upper = cc.trim().to_uppercase();
            if !upper.is_empty() {
                candidates.retain(|s| s.country.eq_ignore_ascii_case(&upper));
            }
        }

        if candidates.is_empty() {
            return None;
        }

        // Pick the server with lowest load
        candidates.sort_by_key(|s| s.load);
        candidates.first().cloned().cloned()
    }

    /// Returns a list of servers matching the specified country and user's tier.
    pub async fn get_servers(
        &self,
        app: &AppContext,
        country: Option<&str>,
    ) -> Vec<ProtonServerSummary> {
        let servers = self.load_cached_servers(app).await;
        let session = self.load_session(app);
        let user_tier = session.as_ref().map(|s| s.max_tier).unwrap_or(0);

        let mut filtered: Vec<ProtonServerSummary> = servers
            .into_iter()
            .filter(|s| s.tier <= user_tier)
            .filter(|s| {
                if let Some(c) = country {
                    let upper = c.trim().to_uppercase();
                    if !upper.is_empty() && s.country != upper {
                        return false;
                    }
                }
                true
            })
            .map(|s| ProtonServerSummary {
                name: s.name,
                country: s.country,
                city: s.city,
                load: s.load,
                tier: s.tier,
            })
            .collect();

        filtered.sort_by(|a, b| a.load.cmp(&b.load).then_with(|| a.name.cmp(&b.name)));
        filtered
    }

    /// Locates the `wireproxy` executable binary.
    pub fn locate_wireproxy(app: &AppContext) -> Result<PathBuf, String> {
        let exe = if cfg!(windows) { "wireproxy.exe" } else { "wireproxy" };
        let mut candidates = Vec::new();

        if let Ok(data_dir) = app.path().app_data_dir() {
            candidates.push(data_dir.join("wireproxy").join(exe));
            candidates.push(data_dir.join(exe));
        }
        candidates.push(PathBuf::from(exe));
        candidates.push(PathBuf::from("/usr/local/bin").join(exe));
        candidates.push(PathBuf::from("/usr/bin").join(exe));

        for candidate in candidates {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        Err(format!("Wireproxy binary '{exe}' not found. Please download or install it in Core Management."))
    }

    /// Starts WireProxy with `wire_proton.conf`.
    pub async fn start_standalone(
        &self,
        app: &AppContext,
        settings: &ProtonSettings,
    ) -> Result<SocketAddr, String> {
        // Ensure not already running
        self.stop().await;

        // Ensure active and valid session (auto-recovers from downtime or session expiration)
        let session = self.ensure_valid_session(app).await?;

        let private_key = session
            .wg_private_key
            .as_deref()
            .ok_or_else(|| "No WireGuard private key found. Please re-register certificate.".to_string())?;

        // Select node matching current user tier
        let user_tier = session.max_tier;
        let mut target_server = self
            .select_server(
                app,
                settings.country.as_deref(),
                settings.server_name.as_deref(),
                user_tier,
                &[],
            )
            .await;

        // Auto failover logic:
        // 1. If a specific server was specified but not found/offline, fallback to best node in same country
        if target_server.is_none() && settings.server_name.is_some() && settings.auto_failover {
            target_server = self
                .select_server(
                    app,
                    settings.country.as_deref(),
                    None,
                    user_tier,
                    &[],
                )
                .await;
        }

        // 2. Auto failover to lowest-load available server in user's tier if requested country is unavailable
        if target_server.is_none() && settings.auto_failover {
            println!(
                "[proton] Country '{}' has no servers for Tier {}, auto-failing over to best available server in Tier {}",
                settings.country.as_deref().unwrap_or(""),
                user_tier,
                user_tier
            );
            target_server = self
                .select_server(
                    app,
                    None,
                    None,
                    user_tier,
                    &[],
                )
                .await;
        }

        let target_server = target_server.ok_or_else(|| {
            format!(
                "No available Proton server found for country '{}' within account tier (Tier {}). Please select an available country from the list or refresh servers.",
                settings.country.as_deref().unwrap_or("ALL"),
                user_tier
            )
        })?;

        let listen_address = settings
            .listen_address
            .as_deref()
            .unwrap_or(DEFAULT_STANDALONE_ADDRESS);
        let listen_port = settings.listen_port.unwrap_or(DEFAULT_STANDALONE_PORT);

        let conf_content = Self::render_wire_proton_conf(&private_key, &target_server, listen_address, listen_port);
        let dir = self.proton_dir(app);
        let conf_path = dir.join(WIRE_PROTON_CONF_FILENAME);
        fs::write(&conf_path, conf_content)
            .map_err(|e| format!("Failed to write {WIRE_PROTON_CONF_FILENAME}: {e}"))?;

        let bin_path = Self::locate_wireproxy(app)?;

        println!(
            "[proton] Starting WireProxy with {} (Server: {}, Endpoint: {}:{}, Bind: {}:{})",
            WIRE_PROTON_CONF_FILENAME,
            target_server.name,
            target_server.entry_ip,
            target_server.endpoint_port,
            listen_address,
            listen_port
        );

        let mut cmd = Command::new(&bin_path);
        cmd.arg("-c").arg(&conf_path);

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let child = cmd.spawn().map_err(|e| format!("Failed to spawn wireproxy process: {e}"))?;

        {
            let mut proc_guard = self.process.lock().await;
            *proc_guard = Some(child);
        }

        self.is_running.store(true, Ordering::SeqCst);
        *self.active_address.lock().await = Some(listen_address.to_string());
        *self.active_port.lock().await = Some(listen_port);
        *self.active_server.lock().await = Some(target_server.name.clone());
        *self.active_country.lock().await = Some(target_server.country.clone());

        // Launch background watchdog task if not already active
        self.ensure_watchdog(app.clone());

        let addr_str = format!("{}:{}", listen_address, listen_port);
        addr_str
            .parse::<SocketAddr>()
            .map_err(|e| format!("Invalid listening address '{addr_str}': {e}"))
    }

    /// Stops the running WireProxy process and clears active status.
    pub async fn stop(&self) {
        let mut proc_guard = self.process.lock().await;
        if let Some(mut child) = proc_guard.take() {
            println!("[proton] Stopping WireProxy process (PID {:?})", child.id());

            #[cfg(unix)]
            {
                let _ = Command::new("kill").arg("-15").arg(child.id().to_string()).output();
                std::thread::sleep(std::time::Duration::from_millis(150));
            }

            let _ = child.kill();
            let _ = child.wait();
        }

        self.is_running.store(false, Ordering::SeqCst);
        *self.active_address.lock().await = None;
        *self.active_port.lock().await = None;
        *self.active_server.lock().await = None;
        *self.active_country.lock().await = None;
    }

    /// Returns whether the WireProxy process is currently running.
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Ensures the background watchdog task is running to auto-renew certificates,
    /// handle recovery after machine downtime, and monitor connection health.
    pub fn ensure_watchdog(&self, app: AppContext) {
        if self.watchdog_running.swap(true, Ordering::SeqCst) {
            return;
        }

        tokio::spawn(async move {
            loop {
                // Check every 10 minutes
                tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;

                let proton = app.proton();

                // If WireProxy is running or a session was previously saved, ensure session and cert validity
                if proton.is_running() || proton.load_session(&app).is_some() {
                    let old_exp = proton.load_session(&app).and_then(|s| s.cert_expires_at).unwrap_or(0);

                    match proton.ensure_valid_session(&app).await {
                        Ok(session) => {
                            let new_exp = session.cert_expires_at.unwrap_or(0);
                            // If certificate was renewed or changed, and WireProxy is running, smoothly reload
                            if new_exp != old_exp && proton.is_running() {
                                println!("[proton watchdog] Certificate renewed, reloading WireProxy service...");
                                let settings = proton.load_settings(&app);
                                let _ = proton.start_standalone(&app, &settings).await;
                            }
                        }
                        Err(e) => {
                            eprintln!("[proton watchdog] Error during session / cert health check: {e}");
                        }
                    }
                }
            }
        });
    }

    /// Gathers status and summary information for the frontend.
    pub async fn get_info(&self, app: &AppContext) -> ProtonInfoResponse {
        let wireproxy_path = Self::locate_wireproxy(app).ok();
        let wireproxy_installed = wireproxy_path.is_some();
        let path_str = wireproxy_path.map(|p| p.to_string_lossy().into_owned());

        let is_running = self.is_running();
        let active_address = self.active_address.lock().await.clone();
        let active_port = self.active_port.lock().await.clone();
        let active_server = self.active_server.lock().await.clone();
        let active_country = self.active_country.lock().await.clone();

        let session = self.load_session(app);
        let session_active = session.is_some();
        let auth_mode = session.as_ref().map(|s| s.auth_mode.clone()).unwrap_or_else(|| "guest".to_string());
        let cert_expires_at = session.as_ref().and_then(|s| s.cert_expires_at);

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as f64;
        let cert_days_remaining = cert_expires_at.map(|exp| {
            let diff = (exp as f64) - now;
            if diff > 0.0 { diff / 86400.0 } else { 0.0 }
        });

        let servers = self.load_cached_servers(app).await;
        let user_tier = session.as_ref().map(|s| s.max_tier).unwrap_or(0);

        // Filter servers matching current user tier (tier <= user_tier)
        let tier_servers: Vec<&ProtonServer> = servers.iter().filter(|s| s.tier <= user_tier).collect();
        let total_servers = tier_servers.len();

        // Group available servers by country matching user's tier
        let mut country_map: HashMap<String, (usize, u32)> = HashMap::new();
        for s in &tier_servers {
            let entry = country_map.entry(s.country.clone()).or_insert((0, 100));
            entry.0 += 1;
            if s.load < entry.1 {
                entry.1 = s.load;
            }
        }

        let mut countries: Vec<ProtonCountrySummary> = country_map
            .into_iter()
            .map(|(code, (count, lowest_load))| ProtonCountrySummary {
                code,
                count,
                lowest_load,
            })
            .collect();
        countries.sort_by(|a, b| a.code.cmp(&b.code));

        let mut servers_summary: Vec<ProtonServerSummary> = tier_servers
            .iter()
            .map(|s| ProtonServerSummary {
                name: s.name.clone(),
                country: s.country.clone(),
                city: s.city.clone(),
                load: s.load,
                tier: s.tier,
            })
            .collect();
        servers_summary.sort_by(|a, b| a.load.cmp(&b.load).then_with(|| a.name.cmp(&b.name)));

        let settings = self.load_settings(app);

        ProtonInfoResponse {
            wireproxy_installed,
            wireproxy_path: path_str,
            is_running,
            active_address,
            active_port,
            active_server,
            active_country,
            session_active,
            auth_mode,
            user_tier,
            cert_expires_at,
            cert_days_remaining,
            total_servers,
            countries,
            servers: servers_summary,
            settings,
        }
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_string_hashcode_is_consistent() {
        assert_eq!(java_string_hashcode(""), 0);
        assert_eq!(java_string_hashcode("a"), 97);
        assert_eq!(java_string_hashcode("hello"), 99162322);
    }

    #[test]
    fn pixel_9_device_challenge_is_valid_json() {
        let hash = java_string_hashcode("test-machine-id-12345");
        let payload = build_challenge_payload(hash);
        assert_eq!(
            payload["Payload"]["vpn-android-v4-challenge-0"]["deviceName"],
            hash
        );
        assert_eq!(
            payload["Payload"]["vpn-android-v4-challenge-0"]["v"],
            SPOOFED_APP_VERSION
        );
    }

    #[test]
    fn generate_vpn_keys_produces_valid_wg_and_pem_keys() {
        let (wg_key, pem) = generate_vpn_keys().expect("Failed to generate keys");

        // Base64 WireGuard key is 44 characters (32 bytes base64 encoded)
        assert_eq!(wg_key.len(), 44);
        let decoded_wg = BASE64_STANDARD.decode(&wg_key).expect("Valid base64 wg key");
        assert_eq!(decoded_wg.len(), 32);

        // Clamping assertions
        assert_eq!(decoded_wg[0] & 7, 0); // lower 3 bits 0 (&= 248)
        assert_eq!(decoded_wg[31] & 128, 0); // top bit 0 (&= 127)
        assert_eq!(decoded_wg[31] & 64, 64); // second bit 1 (|= 64)

        // PEM checks
        assert!(pem.starts_with("-----BEGIN PUBLIC KEY-----"));
        assert!(pem.ends_with("-----END PUBLIC KEY-----\n"));
    }

    #[test]
    fn wire_proton_conf_renders_correct_ini() {
        let server = ProtonServer {
            id: "US-FREE-1".into(),
            name: "US-FREE#1".into(),
            country: "US".into(),
            city: Some("New York".into()),
            tier: 0,
            score: 1.2,
            load: 12,
            entry_ip: "185.159.157.10".into(),
            x25519_public_key: "0123456789abcdef0123456789abcdef0123456789a=".into(),
            endpoint_port: 51820,
        };

        let conf = Proton::render_wire_proton_conf("MY_TEST_PRIVATE_KEY=", &server, "127.0.0.1", 10810);

        assert!(conf.contains("PrivateKey = MY_TEST_PRIVATE_KEY="));
        assert!(conf.contains("Address = 10.2.0.2/32"));
        assert!(conf.contains("PublicKey = 0123456789abcdef0123456789abcdef0123456789a="));
        assert!(conf.contains("Endpoint = 185.159.157.10:51820"));
        assert!(conf.contains("BindAddress = 127.0.0.1:10810"));
    }

    #[test]
    fn corrupted_session_is_handled_gracefully() {
        let temp_dir = std::env::temp_dir().join(format!("proton_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let session_file = temp_dir.join("session.json");

        // Write corrupted JSON
        std::fs::write(&session_file, "{ invalid json data").unwrap();
        assert!(session_file.exists());

        let content = std::fs::read_to_string(&session_file).unwrap();
        let res = serde_json::from_str::<ProtonSession>(&content);
        assert!(res.is_err());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn country_filtering_selects_lowest_load_in_country() {
        let (event_tx, _) = tokio::sync::broadcast::channel(10);
        let ctx = AppContext {
            supervisor: Arc::new(crate::core_supervisor::CoreSupervisor::new()),
            chain: Arc::new(crate::chain::Chain::new()),
            psiphon: Arc::new(crate::psiphon::Psiphon::new()),
            tor: Arc::new(crate::tor::Tor::new()),
            proton: Arc::new(Proton::new()),
            windscribe: Arc::new(crate::windscribe::Windscribe::new()),
            socks_mgr: Arc::new(crate::socks_instance::SocksInstanceManager::new()),
            lan_door: Arc::new(crate::lan_share::LanDoor::default()),
            config_dir: std::path::PathBuf::from("/tmp"),
            data_dir: std::path::PathBuf::from("/tmp"),
            resource_dir: std::path::PathBuf::from("/tmp"),
            log_dir: std::path::PathBuf::from("/tmp"),
            event_tx,
        };

        let proton = Proton::new();
        {
            let mut cache = proton.servers_cache.write().await;
            *cache = vec![
                ProtonServer {
                    id: "US-1".into(),
                    name: "US-FREE#1".into(),
                    country: "US".into(),
                    city: None,
                    tier: 0,
                    score: 1.0,
                    load: 40,
                    entry_ip: "1.1.1.1".into(),
                    x25519_public_key: "key1".into(),
                    endpoint_port: 51820,
                },
                ProtonServer {
                    id: "NL-1".into(),
                    name: "NL-FREE#1".into(),
                    country: "NL".into(),
                    city: None,
                    tier: 0,
                    score: 1.0,
                    load: 85,
                    entry_ip: "2.2.2.1".into(),
                    x25519_public_key: "key2".into(),
                    endpoint_port: 51820,
                },
                ProtonServer {
                    id: "NL-2".into(),
                    name: "NL-FREE#2".into(),
                    country: "NL".into(),
                    city: None,
                    tier: 0,
                    score: 1.0,
                    load: 72,
                    entry_ip: "2.2.2.2".into(),
                    x25519_public_key: "key3".into(),
                    endpoint_port: 51820,
                },
            ];
        }

        // Test 1: Selecting NL with auto server (None) MUST pick NL-FREE#2 (load 72), NOT US-FREE#1 (load 40)
        let s1 = proton.select_server(&ctx, Some("NL"), None, 0, &[]).await;
        assert!(s1.is_some());
        let s1 = s1.unwrap();
        assert_eq!(s1.country, "NL");
        assert_eq!(s1.name, "NL-FREE#2");
        assert_eq!(s1.load, 72);

        // Test 2: Selecting US with auto server (None) picks US-FREE#1 (load 40)
        let s2 = proton.select_server(&ctx, Some("US"), None, 0, &[]).await;
        assert!(s2.is_some());
        let s2 = s2.unwrap();
        assert_eq!(s2.country, "US");
        assert_eq!(s2.name, "US-FREE#1");
        assert_eq!(s2.load, 40);

        // Test 3: If an old server from another country (e.g. US-FREE#1) was requested while country is NL,
        // it must ignore US-FREE#1 and select the lowest load node in NL!
        let s3 = proton.select_server(&ctx, Some("NL"), Some("US-FREE#1"), 0, &[]).await;
        assert!(s3.is_some());
        let s3 = s3.unwrap();
        assert_eq!(s3.country, "NL");
        assert_eq!(s3.name, "NL-FREE#2");
        assert_eq!(s3.load, 72);
    }
}
