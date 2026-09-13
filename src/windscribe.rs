//! Windscribe carrier and standalone proxy manager.
//!
//! Connects to Windscribe's free HTTPS proxy nodes using HTTP CONNECT tunnels over TLS,
//! and provides a local SOCKS5 listener on `127.0.0.1:10809` (or user-defined port).
//!
//! Features:
//! - Register anonymous accounts (with optional email to unlock 10GB/mo free quota)
//! - Login with existing Windscribe username and password
//! - Optional upstream/front proxy for registration/login to avoid datacenter IP rate limits
//! - Query account quota, session status, and server list (13 free locations including HK, US, CA, etc.)
//! - Zero external binary dependencies: pure Rust TLS-bridged SOCKS5 server

use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::prelude::*;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::app_context::AppContext;

pub const WINDSCRIBE_DIR_NAME: &str = "windscribe";
pub const STANDALONE_CONFIG_FILENAME: &str = "standalone_config.json";
pub const ACCOUNT_FILENAME: &str = "account.json";
pub const SERVERS_CACHE_FILENAME: &str = "servers.json";

pub const DEFAULT_STANDALONE_PORT: u16 = 10809;
pub const DEFAULT_STANDALONE_ADDRESS: &str = "127.0.0.1";

pub const CLIENT_AUTH_SECRET: &str = "952b4412f002315aa50751032fcaab03";
pub const API_BASE_URL: &str = "https://api.windscribe.com";
pub const ASSETS_BASE_URL: &str = "https://assets.windscribe.com/serverlist";
pub const PROXY_REMOTE_PORT: u16 = 443;

pub const SPOOFED_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/103.0.5060.53 Safari/537.36";
pub const SPOOFED_ORIGIN: &str = "chrome-extension://hnmpcagpplmpfojmgmnngilcnanddlhb";

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

// ── Pure Rust MD5 Implementation (RFC 1321) ───────────────────────────────────

pub fn md5_hex(data: &[u8]) -> String {
    let mut a: u32 = 0x67452301;
    let mut b: u32 = 0xefcdab89;
    let mut c: u32 = 0x98badcfe;
    let mut d: u32 = 0x10325476;

    let orig_len_bits = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 64);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&orig_len_bits.to_le_bytes());

    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
        0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
        0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
        0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
        0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
        0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
        0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
        0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];

    for chunk in msg.chunks_exact(64) {
        let mut m = [0u32; 16];
        for (i, bytes) in chunk.chunks_exact(4).enumerate() {
            m[i] = u32::from_le_bytes(bytes.try_into().unwrap());
        }

        let mut aa = a;
        let mut bb = b;
        let mut cc = c;
        let mut dd = d;

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((bb & cc) | ((!bb) & dd), i),
                16..=31 => ((dd & bb) | ((!dd) & cc), (5 * i + 1) % 16),
                32..=47 => (bb ^ cc ^ dd, (3 * i + 5) % 16),
                48..=63 => (cc ^ (bb | (!dd)), (7 * i) % 16),
                _ => unreachable!(),
            };
            let temp = dd;
            dd = cc;
            cc = bb;
            bb = bb.wrapping_add(
                (aa.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g])).rotate_left(S[i]),
            );
            aa = temp;
        }

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a.to_le_bytes());
    out[4..8].copy_from_slice(&b.to_le_bytes());
    out[8..12].copy_from_slice(&c.to_le_bytes());
    out[12..16].copy_from_slice(&d.to_le_bytes());

    let mut hex = String::with_capacity(32);
    for byte in out {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{:02x}", byte);
    }
    hex
}

pub fn auth_hash() -> (String, u64) {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let payload = format!("{}{}", CLIENT_AUTH_SECRET, t);
    let hash = md5_hex(payload.as_bytes());
    (hash, t)
}

fn rand_alphanumeric(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    let _ = getrandom::fill(&mut bytes);
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    bytes.iter().map(|b| CHARSET[(*b as usize) % CHARSET.len()] as char).collect()
}

fn rand_password() -> String {
    let mut bytes = [0u8; 16];
    let _ = getrandom::fill(&mut bytes);
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut s: String = bytes.iter().map(|b| CHARSET[(*b as usize) % CHARSET.len()] as char).collect();
    s.push_str("!aA9");
    s
}

// ── Models & State ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct WindscribeSettings {
    pub listen_address: Option<String>,
    pub listen_port: Option<u16>,
    pub country: Option<String>,
    pub server_tag: Option<String>,
    pub upstream_proxy: Option<String>,
    pub auto_failover: bool,
}

impl Default for WindscribeSettings {
    fn default() -> Self {
        Self {
            listen_address: Some(DEFAULT_STANDALONE_ADDRESS.to_string()),
            listen_port: Some(DEFAULT_STANDALONE_PORT),
            country: Some("HK".to_string()),
            server_tag: None,
            upstream_proxy: None,
            auto_failover: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct WindscribeAccount {
    pub username: String,
    pub password: Option<String>,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub session_auth_hash: String,
    pub loc_hash: String,
    pub traffic_max: u64,
    pub traffic_used: u64,
    pub status: i32,
    pub proxy_username: Option<String>,
    pub proxy_password: Option<String>,
    pub registered_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindscribeServer {
    pub tag: String,
    pub loc: String,
    pub loc_name: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindscribeSnapshot {
    pub is_running: bool,
    pub state: String,
    pub active_address: Option<String>,
    pub current_server: Option<WindscribeServer>,
    pub account: Option<WindscribeAccount>,
    pub servers: Vec<WindscribeServer>,
    pub last_error: Option<String>,
    pub status_message: Option<String>,
}

pub fn standalone_config_path(app: &AppContext) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("data"))
        .join(WINDSCRIBE_DIR_NAME)
        .join(STANDALONE_CONFIG_FILENAME)
}

pub fn account_file_path(app: &AppContext) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("data"))
        .join(WINDSCRIBE_DIR_NAME)
        .join(ACCOUNT_FILENAME)
}

pub fn servers_cache_path(app: &AppContext) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("data"))
        .join(WINDSCRIBE_DIR_NAME)
        .join(SERVERS_CACHE_FILENAME)
}

pub fn load_standalone_settings(app: &AppContext) -> WindscribeSettings {
    let path = standalone_config_path(app);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<WindscribeSettings>(&content) {
                return settings;
            }
        }
    }
    WindscribeSettings::default()
}

pub fn save_standalone_settings(app: &AppContext, settings: &WindscribeSettings) -> Result<(), String> {
    let path = standalone_config_path(app);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let rendered = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("cannot serialize windscribe settings: {e}"))?;
    fs::write(&path, &rendered)
        .map_err(|e| format!("cannot save standalone windscribe settings: {e}"))?;
    Ok(())
}

// ── HTTP Client Helper (Supports Upstream Front-Proxy) ────────────────────────

pub fn normalize_proxy_url(proxy_str: &str) -> String {
    let mut trimmed = proxy_str.trim();
    while trimmed.ends_with('/') {
        trimmed = &trimmed[..trimmed.len() - 1];
    }

    let lower = trimmed.to_ascii_lowercase();

    if lower.starts_with("socks5://") {
        format!("socks5h://{}", &trimmed[9..])
    } else if lower.starts_with("socks5h://") {
        trimmed.to_string()
    } else if lower.starts_with("socks4://") {
        format!("socks4a://{}", &trimmed[9..])
    } else if lower.starts_with("socks4a://") {
        trimmed.to_string()
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        // Standard HTTP / HTTPS upstream proxy
        trimmed.to_string()
    } else if !trimmed.contains("://") {
        // If user entered IP:PORT without scheme:
        // Distinguish common HTTP proxy ports vs SOCKS proxy ports
        if let Some((_, port_str)) = trimmed.rsplit_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                if matches!(port, 7890 | 8080 | 8888 | 3128 | 80 | 8000 | 1087 | 8118) {
                    return format!("http://{trimmed}");
                }
            }
        }
        format!("socks5h://{trimmed}")
    } else {
        trimmed.to_string()
    }
}

fn create_http_client(proxy_str: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30));

    if let Some(proxy_url) = proxy_str {
        let trimmed = proxy_url.trim();
        if !trimmed.is_empty() {
            let normalized = normalize_proxy_url(trimmed);
            let proxy = reqwest::Proxy::all(&normalized)
                .map_err(|e| format!("invalid upstream proxy format '{normalized}': {e}"))?;
            builder = builder.proxy(proxy);
        }
    }

    builder.build().map_err(|e| format!("failed to build HTTP client: {e}"))
}

// ── API Operations ────────────────────────────────────────────────────────────

pub async fn register_new_account(
    email: Option<&str>,
    upstream_proxy: Option<&str>,
) -> Result<WindscribeAccount, String> {
    let client = create_http_client(upstream_proxy)?;
    let (h, t) = auth_hash();
    let user = format!("u{}", rand_alphanumeric(9));
    let pw = rand_password();

    let mut form = vec![
        ("client_auth_hash", h),
        ("time", t.to_string()),
        ("session_type_id", "2".to_string()),
        ("username", user.clone()),
        ("password", pw.clone()),
    ];
    if let Some(em) = email {
        let trimmed = em.trim();
        if !trimmed.is_empty() {
            form.push(("email", trimmed.to_string()));
        }
    }

    let mut body_str = String::new();
    for (i, (k, v)) in form.iter().enumerate() {
        if i > 0 {
            body_str.push('&');
        }
        body_str.push_str(k);
        body_str.push('=');
        // Simple percent encoding for form values
        body_str.push_str(&urlencoding_simple(v));
    }

    let resp = client
        .post(format!("{API_BASE_URL}/Users"))
        .header("User-Agent", SPOOFED_USER_AGENT)
        .header("Origin", SPOOFED_ORIGIN)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body_str)
        .send()
        .await
        .map_err(|e| format!("network request to Windscribe failed: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let json_val: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| format!("Windscribe returned non-JSON HTTP {status}: {text}"))?;

    if status.as_u16() == 429 {
        return Err("Rate limited by Windscribe. Please configure an upstream front-proxy or try again later.".to_string());
    }

    let data = json_val.get("data").ok_or_else(|| {
        let msg = json_val
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .or_else(|| json_val.get("message").and_then(|v| v.as_str()))
            .unwrap_or("unknown error");
        format!("Registration failed HTTP {status}: {msg}")
    })?;

    let account_status = data.get("status").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    if account_status != 1 {
        return Err(format!(
            "Windscribe issued a restricted account (status={account_status}, traffic_max={}). \
             This IP has been flagged for frequent registrations. Please configure an upstream proxy and retry.",
            data.get("traffic_max").unwrap_or(&serde_json::Value::Null)
        ));
    }

    let session_auth_hash = data
        .get("session_auth_hash")
        .and_then(|v| v.as_str())
        .ok_or("missing session_auth_hash in response")?
        .to_string();
    let loc_hash = data
        .get("loc_hash")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let user_id = data
        .get("user_id")
        .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string())));
    let traffic_max = data.get("traffic_max").and_then(|v| v.as_u64()).unwrap_or(2 * 1024 * 1024 * 1024);

    let (proxy_user, proxy_pass) = fetch_credentials(&session_auth_hash, upstream_proxy).await?;

    Ok(WindscribeAccount {
        username: user,
        password: Some(pw),
        email: email.map(|s| s.trim().to_string()),
        user_id,
        session_auth_hash,
        loc_hash,
        traffic_max,
        traffic_used: 0,
        status: account_status,
        proxy_username: Some(proxy_user),
        proxy_password: Some(proxy_pass),
        registered_at: Some(chrono_now_iso()),
    })
}

fn urlencoding_simple(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut out, "%{:02X}", b);
        }
    }
    out
}

pub async fn login_existing_account(
    username: &str,
    password: &str,
    upstream_proxy: Option<&str>,
) -> Result<WindscribeAccount, String> {
    let client = create_http_client(upstream_proxy)?;
    let (h, t) = auth_hash();

    let form = [
        ("client_auth_hash", h),
        ("time", t.to_string()),
        ("session_type_id", "2".to_string()),
        ("username", username.trim().to_string()),
        ("password", password.trim().to_string()),
    ];

    let mut body_str = String::new();
    for (i, (k, v)) in form.iter().enumerate() {
        if i > 0 {
            body_str.push('&');
        }
        body_str.push_str(k);
        body_str.push('=');
        body_str.push_str(&urlencoding_simple(v));
    }

    let resp = client
        .post(format!("{API_BASE_URL}/Session"))
        .header("User-Agent", SPOOFED_USER_AGENT)
        .header("Origin", SPOOFED_ORIGIN)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body_str)
        .send()
        .await
        .map_err(|e| format!("failed to reach Windscribe login API: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let json_val: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| format!("Windscribe returned non-JSON HTTP {status}: {text}"))?;

    if status.as_u16() == 429 {
        return Err("Login rate-limited by Windscribe. Please configure an upstream front-proxy or try again later.".to_string());
    }

    let data = json_val.get("data").ok_or_else(|| {
        let msg = json_val
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .or_else(|| json_val.get("message").and_then(|v| v.as_str()))
            .unwrap_or("Invalid username or password");
        format!("Login failed HTTP {status}: {msg}")
    })?;

    let session_auth_hash = data
        .get("session_auth_hash")
        .and_then(|v| v.as_str())
        .ok_or("missing session_auth_hash in response")?
        .to_string();
    let loc_hash = data
        .get("loc_hash")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let user_id = data
        .get("user_id")
        .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string())));
    let traffic_max = data.get("traffic_max").and_then(|v| v.as_u64()).unwrap_or(0);
    let traffic_used = data.get("traffic_used").and_then(|v| v.as_u64()).unwrap_or(0);
    let account_status = data.get("status").and_then(|v| v.as_i64()).unwrap_or(1) as i32;

    let (proxy_user, proxy_pass) = fetch_credentials(&session_auth_hash, upstream_proxy).await?;

    Ok(WindscribeAccount {
        username: username.trim().to_string(),
        password: Some(password.trim().to_string()),
        email: None,
        user_id,
        session_auth_hash,
        loc_hash,
        traffic_max,
        traffic_used,
        status: account_status,
        proxy_username: Some(proxy_user),
        proxy_password: Some(proxy_pass),
        registered_at: None,
    })
}

pub async fn fetch_credentials(
    session_auth_hash: &str,
    upstream_proxy: Option<&str>,
) -> Result<(String, String), String> {
    let client = create_http_client(upstream_proxy)?;
    let (h, t) = auth_hash();

    let url = format!(
        "{API_BASE_URL}/ServerCredentials?client_auth_hash={h}&session_auth_hash={session_auth_hash}&time={t}"
    );

    let resp = client
        .get(url)
        .header("User-Agent", SPOOFED_USER_AGENT)
        .header("Origin", SPOOFED_ORIGIN)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("failed to fetch ServerCredentials: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let json_val: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| format!("non-JSON credentials response HTTP {status}: {text}"))?;

    let data = json_val.get("data").ok_or_else(|| {
        let msg = json_val
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .unwrap_or("credential retrieval refused");
        format!("ServerCredentials error HTTP {status}: {msg}")
    })?;

    let u_b64 = data.get("username").and_then(|v| v.as_str()).ok_or("missing username in credentials")?;
    let p_b64 = data.get("password").and_then(|v| v.as_str()).ok_or("missing password in credentials")?;

    let u_bytes = BASE64_STANDARD.decode(u_b64).map_err(|e| format!("invalid base64 username: {e}"))?;
    let p_bytes = BASE64_STANDARD.decode(p_b64).map_err(|e| format!("invalid base64 password: {e}"))?;

    let username = String::from_utf8_lossy(&u_bytes).to_string();
    let password = String::from_utf8_lossy(&p_bytes).to_string();

    Ok((username, password))
}

pub async fn refresh_session_info(
    account: &mut WindscribeAccount,
    upstream_proxy: Option<&str>,
) -> Result<(), String> {
    let client = create_http_client(upstream_proxy)?;
    let (h, t) = auth_hash();

    let url = format!(
        "{API_BASE_URL}/Session?client_auth_hash={h}&session_auth_hash={}&time={t}&session_type_id=2",
        account.session_auth_hash
    );

    let resp = client
        .get(url)
        .header("User-Agent", SPOOFED_USER_AGENT)
        .header("Origin", SPOOFED_ORIGIN)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("failed to refresh session: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let json_val: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| format!("non-JSON session response HTTP {status}: {text}"))?;

    let data = json_val.get("data").ok_or_else(|| {
        let msg = json_val.get("errorMessage").and_then(|v| v.as_str()).unwrap_or("session fetch error");
        format!("Session error HTTP {status}: {msg}")
    })?;

    if let Some(used) = data.get("traffic_used").and_then(|v| v.as_u64()) {
        account.traffic_used = used;
    }
    if let Some(max) = data.get("traffic_max").and_then(|v| v.as_u64()) {
        account.traffic_max = max;
    }
    if let Some(loc) = data.get("loc_hash").and_then(|v| v.as_str()) {
        if !loc.is_empty() {
            account.loc_hash = loc.to_string();
        }
    }
    if let Some(st) = data.get("status").and_then(|v| v.as_i64()) {
        account.status = st as i32;
    }

    Ok(())
}

pub async fn fetch_server_list(
    loc_hash: &str,
    upstream_proxy: Option<&str>,
) -> Result<Vec<WindscribeServer>, String> {
    let client = create_http_client(upstream_proxy)?;
    let url = format!("{ASSETS_BASE_URL}/chrome/0/{loc_hash}");

    let resp = client
        .get(url)
        .header("User-Agent", SPOOFED_USER_AGENT)
        .header("Origin", SPOOFED_ORIGIN)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("failed to download server list: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("serverlist API returned HTTP {status}"));
    }

    let json_val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("failed to parse serverlist JSON: {e}"))?;

    let loc_names = [
        ("US-C", "美国中部"),
        ("US", "美国东部"),
        ("US-W", "美国西部"),
        ("CA", "加拿大东部"),
        ("CA-W", "加拿大西部"),
        ("FR", "法国"),
        ("DE", "德国"),
        ("NL", "荷兰"),
        ("NO", "挪威"),
        ("RO", "罗马尼亚"),
        ("CH", "瑞士"),
        ("GB", "英国"),
        ("HK", "香港"),
    ];

    let mut servers = Vec::new();
    let entries = json_val.get("data").and_then(|v| v.as_array()).ok_or("serverlist missing 'data' array")?;

    for country_item in entries {
        let is_premium = country_item.get("premium_only").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
        if is_premium {
            continue;
        }

        let short_name = country_item.get("short_name").and_then(|v| v.as_str()).unwrap_or_default();
        let loc_display = loc_names
            .iter()
            .find(|(k, _)| *k == short_name)
            .map(|(_, name)| *name)
            .unwrap_or(short_name);

        let mut seq = 0;
        if let Some(groups) = country_item.get("groups").and_then(|v| v.as_array()) {
            for g in groups {
                let is_pro = g.get("pro").and_then(|v| v.as_i64()).unwrap_or(0) == 1;
                if is_pro {
                    continue;
                }
                let city = g.get("city").and_then(|v| v.as_str()).unwrap_or("");
                let nick = g.get("nick").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(hosts) = g.get("hosts").and_then(|v| v.as_array()) {
                    for h in hosts {
                        if let Some(hostname) = h.get("hostname").and_then(|v| v.as_str()) {
                            seq += 1;
                            let tag_desc = if !city.is_empty() && !nick.is_empty() {
                                format!("{loc_display} - {city} ({nick}) #{seq}")
                            } else if !city.is_empty() {
                                format!("{loc_display} - {city} #{seq}")
                            } else {
                                format!("{loc_display} #{seq}")
                            };
                            servers.push(WindscribeServer {
                                tag: tag_desc,
                                loc: short_name.to_string(),
                                loc_name: loc_display.to_string(),
                                host: hostname.to_string(),
                                port: PROXY_REMOTE_PORT,
                            });
                        }
                    }
                }
            }
        }
    }

    if servers.is_empty() {
        return Err("No free servers found in Windscribe response".to_string());
    }

    Ok(servers)
}

fn chrono_now_iso() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    format!("{now}")
}

// ── SOCKS5 to Windscribe HTTPS Proxy Forwarder ────────────────────────────────

struct BridgeInstance {
    listen_addr: SocketAddr,
    stop_signal: Arc<AtomicBool>,
}

impl BridgeInstance {
    fn stop(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.listen_addr);
    }
}

pub struct Windscribe {
    snapshot: Arc<RwLock<WindscribeSnapshot>>,
    bridge: Arc<RwLock<Option<BridgeInstance>>>,
}

impl Default for Windscribe {
    fn default() -> Self {
        Self::new()
    }
}

impl Windscribe {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(WindscribeSnapshot::default())),
            bridge: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn snapshot(&self) -> WindscribeSnapshot {
        self.snapshot.read().await.clone()
    }

    pub async fn is_running(&self) -> bool {
        self.bridge.read().await.is_some()
    }

    pub async fn load_initial_data(&self, app: &AppContext) {
        let acc_path = account_file_path(app);
        let mut account_opt = None;
        if acc_path.exists() {
            if let Ok(content) = fs::read_to_string(&acc_path) {
                if let Ok(acc) = serde_json::from_str::<WindscribeAccount>(&content) {
                    account_opt = Some(acc);
                }
            }
        }

        let srv_path = servers_cache_path(app);
        let mut servers = Vec::new();
        if srv_path.exists() {
            if let Ok(content) = fs::read_to_string(&srv_path) {
                if let Ok(list) = serde_json::from_str::<Vec<WindscribeServer>>(&content) {
                    servers = list;
                }
            }
        }

        let mut snap = self.snapshot.write().await;
        snap.account = account_opt;
        snap.servers = servers;
        snap.state = "idle".to_string();
    }

    pub async fn save_account(&self, app: &AppContext, acc: &WindscribeAccount) -> Result<(), String> {
        let path = account_file_path(app);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(acc).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| format!("cannot write account file: {e}"))?;

        let mut snap = self.snapshot.write().await;
        snap.account = Some(acc.clone());
        Ok(())
    }

    pub async fn save_servers(&self, app: &AppContext, servers: &[WindscribeServer]) -> Result<(), String> {
        let path = servers_cache_path(app);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(servers).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| format!("cannot write servers cache: {e}"))?;

        let mut snap = self.snapshot.write().await;
        snap.servers = servers.to_vec();
        Ok(())
    }

    pub async fn start(
        &self,
        app: &AppContext,
        settings: &WindscribeSettings,
    ) -> Result<SocketAddr, String> {
        self.stop().await;

        let snap_read = self.snapshot.read().await;
        let account = snap_read.account.as_ref().ok_or("No Windscribe account logged in. Please login or register first.")?;
        let proxy_user = account.proxy_username.as_ref().ok_or("Account missing proxy username")?.clone();
        let proxy_pass = account.proxy_password.as_ref().ok_or("Account missing proxy password")?.clone();

        let servers = &snap_read.servers;
        if servers.is_empty() {
            return Err("No servers available. Please refresh the server list.".to_string());
        }

        // Pick requested server or country
        let chosen_server = if let Some(ref tag) = settings.server_tag {
            servers.iter().find(|s| &s.tag == tag).cloned()
        } else if let Some(ref cc) = settings.country {
            servers.iter().find(|s| &s.loc == cc).cloned()
        } else {
            None
        }.or_else(|| servers.first().cloned()).ok_or("No valid server found to connect")?;

        drop(snap_read);

        let listen_ip = settings.listen_address.as_deref().unwrap_or(DEFAULT_STANDALONE_ADDRESS);
        let listen_port = settings.listen_port.unwrap_or(DEFAULT_STANDALONE_PORT);
        let listen_addr: SocketAddr = format!("{listen_ip}:{listen_port}")
            .parse()
            .map_err(|e| format!("Invalid listen address '{listen_ip}:{listen_port}': {e}"))?;

        let listener = TcpListener::bind(listen_addr)
            .map_err(|e| format!("Failed to bind SOCKS5 listener on {listen_addr}: {e}"))?;
        let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

        let stop_signal = Arc::new(AtomicBool::new(false));
        let bridge_instance = BridgeInstance {
            listen_addr: bound_addr,
            stop_signal: stop_signal.clone(),
        };

        *self.bridge.write().await = Some(bridge_instance);

        {
            let mut snap = self.snapshot.write().await;
            snap.is_running = true;
            snap.state = "connected".to_string();
            snap.active_address = Some(bound_addr.to_string());
            snap.current_server = Some(chosen_server.clone());
            snap.last_error = None;
            snap.status_message = Some(format!("Forwarding to {}", chosen_server.tag));
        }

        // Build TLS roots for connecting to Windscribe nodes
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let client_config = Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );

        let srv_host = chosen_server.host.clone();
        let srv_port = chosen_server.port;
        let auth_basic = BASE64_STANDARD.encode(format!("{proxy_user}:{proxy_pass}"));

        // Spawn accept loop thread
        thread::Builder::new()
            .name("windscribe-socks5-bridge".into())
            .spawn(move || {
                for client in listener.incoming() {
                    if stop_signal.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(client) = client else { continue };
                    let config = client_config.clone();
                    let s_host = srv_host.clone();
                    let basic = auth_basic.clone();

                    thread::spawn(move || {
                        let _ = handle_socks5_client(client, &s_host, srv_port, &basic, config);
                    });
                }
            })
            .map_err(|e| format!("failed to spawn bridge listener thread: {e}"))?;

        app.supervisor().record(
            "windscribe",
            "info",
            format!("Windscribe SOCKS5 listener started on {bound_addr}, outbound to {}", chosen_server.tag),
        );

        Ok(bound_addr)
    }

    pub async fn stop(&self) {
        if let Some(instance) = self.bridge.write().await.take() {
            instance.stop();
        }
        let mut snap = self.snapshot.write().await;
        snap.is_running = false;
        snap.state = "idle".to_string();
        snap.active_address = None;
        snap.status_message = None;
    }
}

// ── SOCKS5 Handshake & TLS CONNECT Forwarding ─────────────────────────────────

fn handle_socks5_client(
    mut client: TcpStream,
    ws_host: &str,
    ws_port: u16,
    auth_basic: &str,
    tls_config: Arc<ClientConfig>,
) -> io::Result<()> {
    client.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    client.set_nodelay(true)?;

    // 1. SOCKS5 Greeting
    let mut header = [0u8; 2];
    client.read_exact(&mut header)?;
    if header[0] != 0x05 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Not SOCKS5"));
    }
    let num_methods = header[1] as usize;
    let mut methods = vec![0u8; num_methods];
    client.read_exact(&mut methods)?;

    // Accept "NO AUTHENTICATION REQUIRED" (0x00)
    client.write_all(&[0x05, 0x00])?;

    // 2. SOCKS5 Request
    let mut req_header = [0u8; 4];
    client.read_exact(&mut req_header)?;
    if req_header[0] != 0x05 || req_header[1] != 0x01 {
        // Only CONNECT is supported
        let _ = client.write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        return Err(io::Error::new(io::ErrorKind::Unsupported, "Only CONNECT command supported"));
    }

    let target_host = match req_header[3] {
        0x01 => {
            // IPv4
            let mut ip = [0u8; 4];
            client.read_exact(&mut ip)?;
            Ipv4Addr::from(ip).to_string()
        }
        0x03 => {
            // Domain
            let mut len = [0u8; 1];
            client.read_exact(&mut len)?;
            let mut domain = vec![0u8; len[0] as usize];
            client.read_exact(&mut domain)?;
            String::from_utf8_lossy(&domain).to_string()
        }
        0x04 => {
            // IPv6
            let mut ip = [0u8; 16];
            client.read_exact(&mut ip)?;
            format!("[{}]", std::net::Ipv6Addr::from(ip))
        }
        _ => {
            let _ = client.write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Unknown address type"));
        }
    };

    let mut port_bytes = [0u8; 2];
    client.read_exact(&mut port_bytes)?;
    let target_port = u16::from_be_bytes(port_bytes);

    // 3. Connect to Windscribe node via TLS
    let target_authority = format!("{target_host}:{target_port}");
    let tcp_out = TcpStream::connect((ws_host, ws_port))?;
    tcp_out.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    tcp_out.set_nodelay(true)?;

    let server_name = ServerName::try_from(ws_host.to_string())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("Invalid SNI '{ws_host}': {e}")))?;

    let conn = ClientConnection::new(tls_config, server_name)
        .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, format!("TLS handshake failed: {e}")))?;

    let mut tls_stream = StreamOwned::new(conn, tcp_out);

    // 4. Send HTTP CONNECT tunnel request to Windscribe node
    let connect_req = format!(
        "CONNECT {target_authority} HTTP/1.1\r\n\
         Host: {target_authority}\r\n\
         Proxy-Authorization: Basic {auth_basic}\r\n\
         User-Agent: {SPOOFED_USER_AGENT}\r\n\
         Proxy-Connection: Keep-Alive\r\n\r\n"
    );

    tls_stream.write_all(connect_req.as_bytes())?;
    tls_stream.flush()?;

    // 5. Read HTTP response from Windscribe node
    let mut resp_buf = Vec::new();
    let mut single_byte = [0u8; 1];
    while resp_buf.len() < 4096 {
        tls_stream.read_exact(&mut single_byte)?;
        resp_buf.push(single_byte[0]);
        if resp_buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    let resp_text = String::from_utf8_lossy(&resp_buf);
    let first_line = resp_text.lines().next().unwrap_or_default();
    if !first_line.contains("200") {
        let _ = client.write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        return Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("Windscribe node rejected CONNECT: {first_line}"),
        ));
    }

    // 6. Respond to local client: SOCKS5 success
    client.write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0x2a, 0x39])?;
    client.set_read_timeout(None)?;

    // 7. Bidirectional Splice
    splice_tls(client, tls_stream);
    Ok(())
}

fn splice_tls(client: TcpStream, tls: StreamOwned<ClientConnection, TcpStream>) {
    let Ok(client_read) = client.try_clone() else { return };
    let (mut c_r, mut c_w) = (client_read, client);

    let tls = Arc::new(std::sync::Mutex::new(tls));
    let tls_out = tls.clone();
    let running = Arc::new(AtomicBool::new(true));
    let r1 = running.clone();

    // Outbound: Client -> TLS
    let outbound = thread::spawn(move || {
        let mut buf = [0u8; 16384];
        while r1.load(Ordering::Relaxed) {
            match c_r.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut guard = match tls_out.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    if guard.write_all(&buf[..n]).is_err() || guard.flush().is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        r1.store(false, Ordering::Relaxed);
    });

    // Inbound: TLS -> Client
    let mut buf = [0u8; 16384];
    while running.load(Ordering::Relaxed) {
        let n = {
            let mut guard = match tls.lock() {
                Ok(g) => g,
                Err(_) => break,
            };
            match guard.read(&mut buf) {
                Ok(n) => n,
                Err(_) => break,
            }
        };
        if n == 0 {
            break;
        }
        if c_w.write_all(&buf[..n]).is_err() || c_w.flush().is_err() {
            break;
        }
    }
    running.store(false, Ordering::Relaxed);
    let _ = c_w.shutdown(Shutdown::Both);
    let _ = outbound.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5_rfc1321_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(b"message digest"),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
        assert_eq!(
            md5_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
    }

    #[test]
    fn test_auth_hash_generates_valid_hex() {
        let (hash, t) = auth_hash();
        assert_eq!(hash.len(), 32);
        assert!(t > 1700000000);
        let expected = md5_hex(format!("{}{}", CLIENT_AUTH_SECRET, t).as_bytes());
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_normalize_proxy_url() {
        assert_eq!(
            normalize_proxy_url("socks5://192.168.6.1:10801"),
            "socks5h://192.168.6.1:10801"
        );
        assert_eq!(
            normalize_proxy_url("SOCKS5://192.168.6.1:10801/"),
            "socks5h://192.168.6.1:10801"
        );
        assert_eq!(
            normalize_proxy_url("socks5h://192.168.6.1:10801"),
            "socks5h://192.168.6.1:10801"
        );
        assert_eq!(
            normalize_proxy_url("http://127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_url("HTTP://127.0.0.1:7890/"),
            "HTTP://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_url("https://127.0.0.1:8443"),
            "https://127.0.0.1:8443"
        );
        assert_eq!(
            normalize_proxy_url("127.0.0.1:7890"),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_url("192.168.6.1:8080"),
            "http://192.168.6.1:8080"
        );
        assert_eq!(
            normalize_proxy_url("192.168.6.1:10801"),
            "socks5h://192.168.6.1:10801"
        );
    }

    #[test]
    fn test_create_http_client_with_http() {
        let client = create_http_client(Some("http://127.0.0.1:7890"));
        assert!(client.is_ok(), "Client should build with http proxy: {:?}", client.err());
    }

    #[test]
    fn test_create_http_client_with_socks5() {
        let client = create_http_client(Some("socks5://192.168.6.1:10801"));
        assert!(client.is_ok(), "Client should build with socks5 proxy: {:?}", client.err());
    }

    #[tokio::test]
    async fn test_socks5_live_request_to_windscribe() {
        let client = create_http_client(Some("socks5://192.168.6.1:10801"))
            .expect("failed to create client");
        let resp = client.get(format!("{API_BASE_URL}/")).send().await;
        if let Ok(r) = resp {
            assert!(r.status().is_success() || r.status().as_u16() == 404 || r.status().as_u16() == 429);
        }
    }
}
