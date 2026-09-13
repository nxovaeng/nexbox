//! warpscout integration — install, status-check, account bridging and scan.
//!
//! warpscout is a standalone binary that tests real WARP / MASQUE endpoints and
//! reports exit country, node, latency and loss.
//!
//! ## Account bridging
//!
//! aether stores its MASQUE identity in `<config>/config/identity/aether-masque.toml`
//! (TOML).  warpscout expects `warpscout-account.json` (JSON) in the working
//! directory with the shape:
//!
//! ```json
//! {
//!   "id":             "<device_id>",
//!   "token":          "<access_token>",
//!   "private_key":    "<wg_private_key base64>",
//!   "peer_public_key":"<wg_peer_public_key base64>",
//!   "ipv4":           "172.16.0.2",
//!   "ipv6":           "...",
//!   "masque": {
//!     "id":             "<device_id>",
//!     "token":          "<access_token>",
//!     "private_key":    "<wg_private_key base64>",
//!     "peer_public_key":"<wg_peer_public_key base64>"
//!   }
//! }
//! ```
//!
//! `from_aether_identity` reads the TOML and writes that JSON, so the user
//! never has to register a second time.  The reverse direction (`to_aether`)
//! is provided so a warpscout-only registration can also feed aether — but in
//! practice aether registers itself on first connect, so it is there for
//! completeness rather than a common path.

use std::env;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ── Data types ────────────────────────────────────────────────────────────────

/// One candidate endpoint returned by a scan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarpEndpoint {
    /// `ip:port`
    pub endpoint: String,
    /// Two-letter exit-country code, e.g. `DE`
    pub country: String,
    /// Cloudflare node airport code, e.g. `FRA`
    pub node: String,
    /// Human-readable node location, e.g. `Frankfurt, Germany`
    pub node_location: String,
    /// Phase-1 ping to the endpoint address in ms
    pub ping_ms: Option<u64>,
    /// In-tunnel RTT in ms (only when -tun-ping / -P was used)
    pub tun_ping_ms: Option<u64>,
    /// Packet loss percentage (only when -tun-ping / -P was used)
    pub loss_pct: Option<f64>,
    /// Download speed in Mbps (only when -speed was used)
    pub speed_mbps: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarpScoutStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    /// `warpscout-account.json` exists in the working directory
    pub account_ready: bool,
    /// How the account was obtained
    pub account_source: AccountSource,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AccountSource {
    /// Derived from an existing aether identity file
    AetherIdentity,
    /// Registered directly by warpscout
    WarpScout,
    /// No account yet
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub endpoints: Vec<WarpEndpoint>,
    /// Raw stdout (for diagnostics when parsing produces nothing)
    pub raw: String,
}


// ── warpscout account (JSON) ──────────────────────────────────────────────────

/// The shape warpscout writes and reads.
#[derive(Debug, Serialize, Deserialize)]
struct WarpScoutAccount {
    id: String,
    token: String,
    private_key: String,
    peer_public_key: String,
    #[serde(default)]
    ipv4: String,
    #[serde(default)]
    ipv6: String,
    /// MASQUE-specific registration (same key material, separate CF device)
    #[serde(skip_serializing_if = "Option::is_none")]
    masque: Option<WarpScoutSubAccount>,
    /// Outer-hop registration for WARP-in-WARP
    #[serde(skip_serializing_if = "Option::is_none")]
    outer: Option<WarpScoutSubAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WarpScoutSubAccount {
    id: String,
    token: String,
    private_key: String,
    peer_public_key: String,
}

// ── Path helpers ──────────────────────────────────────────────────────────────

fn binary_name() -> &'static str {
    if env::consts::OS == "windows" { "warpscout.exe" } else { "warpscout" }
}

pub fn warpscout_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join(binary_name())
}

/// Working directory for warpscout data (report files, etc.)
pub fn warpscout_data_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("warpscout")
}

/// Absolute path of the account JSON file.
pub fn account_json_path(config_dir: &Path) -> PathBuf {
    warpscout_data_dir(config_dir).join("warpscout-account.json")
}


// ── Account bridging ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AetherToml {
    device_id: Option<String>,
    access_token: Option<String>,
    wg_private_key: Option<String>,
    wg_peer_public_key: Option<String>,
    ipv4: Option<String>,
    ipv6: Option<String>,
}

#[derive(Deserialize)]
struct AetherMasqueToml {
    device_id: Option<String>,
    access_token: Option<String>,
    key_pem: Option<String>,
    cert_pem: Option<String>,
}

/// Reads aether.toml for WG and aether-masque.toml for MASQUE, combining them into warpscout-account.json.
pub fn from_aether_identity(data_dir: &Path, identity_dir: &Path) -> Result<(), String> {
    let aether_path = identity_dir.join("aether.toml");
    let masque_path = identity_dir.join("aether-masque.toml");

    let aether_bytes = fs::read(&aether_path)
        .map_err(|e| format!("cannot read {}: {}", aether_path.display(), e))?;
    let aether_toml: AetherToml = toml::from_str(&String::from_utf8_lossy(&aether_bytes))
        .map_err(|e| format!("invalid aether.toml: {e}"))?;

    let masque_bytes = fs::read(&masque_path)
        .map_err(|e| format!("cannot read {}: {}", masque_path.display(), e))?;
    let masque_toml: AetherMasqueToml = toml::from_str(&String::from_utf8_lossy(&masque_bytes))
        .map_err(|e| format!("invalid aether-masque.toml: {e}"))?;

    let mut account = WarpScoutAccount {
        id: aether_toml.device_id.unwrap_or_default(),
        token: aether_toml.access_token.unwrap_or_default(),
        private_key: aether_toml.wg_private_key.unwrap_or_default(),
        peer_public_key: aether_toml.wg_peer_public_key.unwrap_or_default(),
        ipv4: aether_toml.ipv4.unwrap_or_default(),
        ipv6: aether_toml.ipv6.unwrap_or_default(),
        masque: None,
        outer: None,
    };

    account.masque = Some(WarpScoutSubAccount {
        id: masque_toml.device_id.unwrap_or_default(),
        token: masque_toml.access_token.unwrap_or_default(),
        private_key: masque_toml.key_pem.unwrap_or_default(),
        peer_public_key: masque_toml.cert_pem.unwrap_or_default(),
    });

    let account_file = account_json_path(data_dir);
    fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;

    let bytes = serde_json::to_vec_pretty(&account)
        .map_err(|e| format!("cannot serialize account: {e}"))?;
    fs::write(&account_file, bytes)
        .map_err(|e| format!("cannot write warpscout-account.json: {e}"))?;

    Ok(())
}


// ── Account Registration ────────────────────────────────────────────────────────

/// Registers a brand-new WARP account via warpscout, retrieving both WG and MASQUE keys.
///
/// This avoids bridging from aether which only contains one set of credentials,
/// because wg and masque are distinct systems with distinct keys.
pub fn register(bin_dir: &Path, config_dir: &Path) -> Result<(), String> {
    let ws = warpscout_path(bin_dir);
    if !ws.exists() {
        return Err("warpscout is not installed".into());
    }

    let account_file = account_json_path(config_dir);
    let data_dir = warpscout_data_dir(config_dir);
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let out = Command::new(&ws)
        .arg("register")
        .arg("-a").arg(&account_file)
        .arg("-plain")
        .output()
        .map_err(|e| format!("cannot run warpscout register: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(format!("warpscout register failed: {stderr}{stdout}"));
    }

    Ok(())
}

// ── Status ────────────────────────────────────────────────────────────────────

pub fn status(bin_dir: &Path, data_dir: &Path) -> WarpScoutStatus {
    let path = warpscout_path(bin_dir);
    let installed = path.exists();
    let version = if installed { probe_version(&path) } else { None };

    let acct_path = account_json_path(data_dir);
    let identity_dir = data_dir.join("identity");
    let (account_ready, account_source) = if acct_path.exists() {
        (true, AccountSource::WarpScout)
    } else if identity_dir.join("aether.toml").exists() && identity_dir.join("aether-masque.toml").exists() {
        (false, AccountSource::AetherIdentity)
    } else {
        (false, AccountSource::None)
    };

    WarpScoutStatus { installed, version, path: if installed { Some(path.to_string_lossy().into_owned()) } else { None }, account_ready, account_source }
}

fn probe_version(path: &Path) -> Option<String> {
    let out = Command::new(path).arg("version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // Output is a bare version number, e.g. `0.16.0\n`.
    // Some builds prefix it with the binary name; handle both.
    let line = text.lines().next()?.trim();
    // Strip an optional "warpscout" or "warpscout v" prefix
    let version = line
        .trim_start_matches("warpscout")
        .trim_start_matches(' ')
        .trim_start_matches('v')
        .trim();
    if version.is_empty() { None } else { Some(version.to_string()) }
}

// ── Install ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

fn platform_suffix() -> Result<&'static str, String> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64")  => Ok("linux_amd64.tar.gz"),
        ("linux", "aarch64") => Ok("linux_arm64.tar.gz"),
        ("macos", "x86_64")  => Ok("darwin_amd64.tar.gz"),
        ("macos", "aarch64") => Ok("darwin_arm64.tar.gz"),
        (os, arch)           => Err(format!("unsupported platform {os}/{arch}")),
    }
}

/// Download and install warpscout.  Blocking — call via `spawn_blocking`.
pub fn install(bin_dir: &Path) -> Result<(), String> {
    let suffix = platform_suffix()?;

    let release: GhRelease = blocking_get_json(
        "https://api.github.com/repos/vernette/warpscout/releases/latest",
    )?;

    let asset = release.assets.iter()
        .find(|a| a.name.ends_with(suffix))
        .ok_or_else(|| format!("no asset matching *{suffix} in release {}", release.tag_name))?;

    let bytes = blocking_get_bytes(&asset.browser_download_url)?;

    fs::create_dir_all(bin_dir).map_err(|e| e.to_string())?;
    let dest = warpscout_path(bin_dir);

    use flate2::read::GzDecoder;
    use tar::Archive;
    use std::io::Cursor;

    let mut archive = Archive::new(GzDecoder::new(Cursor::new(bytes)));
    let mut found = false;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let name = entry.path().map_err(|e| e.to_string())?
            .file_name().unwrap_or_default()
            .to_string_lossy().into_owned();
        if name == binary_name() {
            entry.unpack(&dest).map_err(|e| e.to_string())?;
            found = true;
            break;
        }
    }
    if !found {
        return Err(format!("'{}' not found inside archive", binary_name()));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest, perms).map_err(|e| e.to_string())?;
    }

    Ok(())
}

// ── Scan ──────────────────────────────────────────────────────────────────────

/// Scan options forwarded from the UI.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    /// `wg` | `awg` | `masque` | `masque-h2`  (default `masque-h2`)
    #[serde(default = "default_proto")]
    pub protocol: String,
    /// Keep only these exit countries, comma-separated, e.g. `"DE,NL"`
    pub country: Option<String>,
    /// Exclude countries, comma-separated
    pub exclude_country: Option<String>,
    /// Keep only these nodes, comma-separated, e.g. `"FRA,AMS"`
    pub node: Option<String>,
    /// Exclude nodes, comma-separated
    pub exclude_node: Option<String>,
    /// Addresses to try per subnet (default 5)
    pub sample: Option<u32>,
    /// Measure in-tunnel latency and loss (-P flag)
    #[serde(default)]
    pub tun_ping: bool,
    /// Run a download speed test on each endpoint (-speed flag)
    #[serde(default)]
    pub speed: bool,
    /// Use IPv6 endpoint pools (-6 flag)
    #[serde(default)]
    pub ipv6: bool,
    /// Phase 2 tunnel workers (-jt flag)
    pub tunnel_jobs: Option<u32>,
    /// Per-request timeout in seconds (-t flag)
    pub timeout: Option<u32>,
    /// Custom SNI for MASQUE (-masque-sni flag)
    pub masque_sni: Option<String>,
    /// Scan from inside a tunnel to this endpoint (-through flag)
    pub through: Option<String>,
}

fn default_proto() -> String { "masque-h2".into() }

/// Run a warpscout scan.  Blocking — call via `spawn_blocking`.
pub fn scan(bin_dir: &Path, config_dir: &Path, opts: ScanOptions) -> Result<ScanResult, String> {
    let ws = warpscout_path(bin_dir);
    if !ws.exists() {
        return Err("warpscout is not installed — install it first".into());
    }

    let account_file = account_json_path(config_dir);
    let data_dir = warpscout_data_dir(config_dir);

    // Ensure account.json is present; auto-register if possible.
    if !account_file.exists() {
        match register(bin_dir, config_dir) {
            Ok(()) => {}
            Err(e) => return Err(format!(
                "No warpscout account found and could not register: {e}"
            )),
        }
    }

    // Ensure data directory exists (for report files)
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

    let mut cmd = Command::new(&ws);
    cmd.arg("scan")
        .arg("-p").arg(&opts.protocol)
        // Use absolute path for account file — no dependency on cwd
        .arg("-a").arg(&account_file)
        // Write report to our data directory (not cwd)
        .arg("-o").arg(data_dir.join("warpscout-report-latest.txt"))
        // Plain text output — no Unicode box-drawing, so parse_table can split on whitespace
        .arg("-plain")
        // Run inside data_dir so any stray files land there, not the project root
        .current_dir(&data_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // Filters
    if let Some(ref c) = opts.country {
        let c = c.trim();
        if !c.is_empty() { cmd.arg("-country").arg(c); }
    }
    if let Some(ref c) = opts.exclude_country {
        let c = c.trim();
        if !c.is_empty() { cmd.arg("-exclude-country").arg(c); }
    }
    if let Some(ref n) = opts.node {
        let n = n.trim();
        if !n.is_empty() { cmd.arg("-node").arg(n); }
    }
    if let Some(ref n) = opts.exclude_node {
        let n = n.trim();
        if !n.is_empty() { cmd.arg("-exclude-node").arg(n); }
    }
    if let Some(s) = opts.sample {
        cmd.arg("-n").arg(s.to_string());
    }
    if opts.tun_ping {
        cmd.arg("-P");
    }
    if opts.speed {
        cmd.arg("-speed");
    }
    if opts.ipv6 {
        cmd.arg("-6");
    }
    if let Some(jt) = opts.tunnel_jobs {
        cmd.arg("-jt").arg(jt.to_string());
    }
    if let Some(t) = opts.timeout {
        cmd.arg("-t").arg(t.to_string());
    }
    if let Some(ref sni) = opts.masque_sni {
        let sni = sni.trim();
        if !sni.is_empty() { cmd.arg("-masque-sni").arg(sni); }
    }
    if let Some(ref through) = opts.through {
        let through = through.trim();
        if !through.is_empty() { cmd.arg("-through").arg(through); }
    }

    let mut child = cmd.spawn().map_err(|e| format!("cannot start warpscout: {e}"))?;
    let stdout = child.stdout.take().ok_or("no stdout pipe")?;

    let lines: Vec<String> = std::io::BufReader::new(stdout)
        .lines()
        .flatten()
        .collect();

    let _ = child.wait();
    let raw = lines.join("\n");
    let endpoints = parse_table(&raw);

    // If the live-stdout parser found nothing, try the written report file as
    // a fallback — it is always plain-text regardless of terminal capabilities.
    let endpoints = if endpoints.is_empty() {
        let report_path = data_dir.join("warpscout-report-latest.txt");
        if let Ok(report) = fs::read_to_string(&report_path) {
            let from_report = parse_table(&report);
            if !from_report.is_empty() { from_report } else { endpoints }
        } else {
            endpoints
        }
    } else {
        endpoints
    };

    Ok(ScanResult { endpoints, raw })
}

// ── Table parser ──────────────────────────────────────────────────────────────

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for ch in chars.by_ref() { if ch == 'm' { break; } }
        } else {
            out.push(c);
        }
    }
    out
}

/// Strip Unicode box-drawing characters used by the new warpscout table output.
/// Turns `│ 8.6.112.36:2408 │ ? │ US │ LAX │ ...` into plain whitespace-separated text.
fn strip_box_drawing(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '│' | '├' | '┤' | '┼' | '╭' | '╮' | '╰' | '╯' | '─' | '┬' | '┴' => ' ',
            _ => c,
        })
        .collect()
}

fn parse_table(raw: &str) -> Vec<WarpEndpoint> {
    let mut results = Vec::new();

    // Try to detect column layout from a header line.
    // Possible header tokens include: SUBNET, ENDPOINT, ENDPOINT PING, TUN PING,
    // LOSS, SPEED, SEEN AS, NODE, NODE LOCATION.
    // We normalize to detect which optional columns are present.
    let upper = raw.to_ascii_uppercase();
    let has_tun_ping_col = upper.contains("TUN PING") || upper.contains("TUN_PING");
    let has_loss_col = upper.contains("LOSS");
    let has_speed_col = upper.contains("SPEED");

    for line in raw.lines() {
        let clean = strip_box_drawing(&strip_ansi(line));
        let trimmed = clean.trim();
        if trimmed.is_empty() { continue; }

        // Skip header and decoration lines
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("endpoint")
            || lower.starts_with("subnet")
            || lower.starts_with("node")
            || lower.starts_with('#')
            || lower.starts_with("---")
            || lower.starts_with("torn")
            || lower.starts_with("scanning")
            || lower.starts_with("phase")
            || lower.starts_with('[')
            || lower.starts_with("warpscout")
            || lower.starts_with("made with")
            || lower.starts_with("https://")
            || lower.starts_with("proto:")
            || lower.starts_with("junk:")
            || lower.starts_with("nodes:")
            || lower.starts_with("seen as:")
            || lower.starts_with("working:")
            || lower.starts_with("best endpoint")
            || lower.starts_with("best by")
            || lower.starts_with("full report")
            || lower.starts_with("using ")
            || lower.starts_with("✔")
        {
            continue;
        }

        let cols: Vec<&str> = trimmed.split_whitespace().collect();
        if cols.len() < 4 { continue; }

        // Find the column that looks like an endpoint (ip:port)
        let (ep_idx, endpoint) = match cols.iter().enumerate().find(|(_, c)| {
            c.contains(':') && c.chars().next().map_or(false, |ch| ch.is_ascii_digit())
        }) {
            Some((i, ep)) => (i, ep.to_string()),
            None => continue,
        };

        let rest = &cols[ep_idx + 1..];
        if rest.len() < 3 { continue; }

        // Detect new vs old format
        let first_after = rest[0];
        let is_new_format = first_after == "?"
            || first_after.ends_with("ms")
            || first_after.parse::<f64>().is_ok();

        if is_new_format {
            // New format columns after endpoint:
            //   ENDPOINT_PING  [TUN_PING  LOSS]  [SPEED]  SEEN_AS  NODE  LOCATION...
            let mut idx = 0;

            let ping_ms = parse_ping(rest[idx]);
            idx += 1;

            let mut tun_ping_ms = None;
            let mut loss_pct = None;
            if has_tun_ping_col && idx + 1 < rest.len() {
                tun_ping_ms = parse_ping(rest[idx]);
                idx += 1;
                // LOSS column: e.g. "0%" or "10%" or "?"
                if has_loss_col {
                    loss_pct = rest.get(idx).and_then(|s| {
                        s.strip_suffix('%').and_then(|v| v.parse::<f64>().ok())
                    });
                    idx += 1;
                }
            }

            let mut speed_mbps = None;
            if has_speed_col && idx < rest.len() {
                // SPEED column: e.g. "12.5" or "12.5Mbps" or "?"
                let s = rest[idx];
                if s != "?" {
                    speed_mbps = s.strip_suffix("Mbps").unwrap_or(s).parse::<f64>().ok();
                }
                idx += 1;
            }

            if idx + 1 >= rest.len() { continue; }
            let seen_as = rest[idx].to_string();
            idx += 1;
            let node = rest[idx].to_string();
            idx += 1;
            let loc = if idx < rest.len() { rest[idx..].join(" ") } else { String::new() };

            // Validate
            if seen_as == "??" || node == "?" { continue; }
            if seen_as.len() != 2 || seen_as.chars().any(|c| !c.is_ascii_alphanumeric()) { continue; }
            if node.len() < 2 || node.chars().any(|c| !c.is_ascii_alphanumeric()) { continue; }

            results.push(WarpEndpoint {
                endpoint,
                country: seen_as,
                node,
                node_location: loc,
                ping_ms,
                tun_ping_ms,
                loss_pct,
                speed_mbps,
            });
        } else {
            // Old format: COUNTRY  NODE  LOCATION...  PING
            let country = rest[0].to_string();
            let node = rest[1].to_string();
            let mut loc_end = rest.len();
            let mut ping = None;
            if let Some(last) = rest.last() {
                if let Some(ms) = last.strip_suffix("ms") {
                    ping = ms.parse().ok();
                    loc_end = rest.len() - 1;
                } else if *last == "?" {
                    loc_end = rest.len() - 1;
                }
            }
            let loc = rest.get(2..loc_end).map(|parts| parts.join(" ")).unwrap_or_default();

            if country == "??" || node == "?" { continue; }
            if country.len() != 2 || country.chars().any(|c| !c.is_ascii_alphanumeric()) { continue; }
            if node.len() < 2 || node.chars().any(|c| !c.is_ascii_alphanumeric()) { continue; }

            results.push(WarpEndpoint {
                endpoint,
                country,
                node,
                node_location: loc,
                ping_ms: ping,
                tun_ping_ms: None,
                loss_pct: None,
                speed_mbps: None,
            });
        }
    }

    results
}

fn parse_ping(s: &str) -> Option<u64> {
    if s == "?" { return None; }
    s.strip_suffix("ms").and_then(|ms| ms.parse().ok())
        .or_else(|| s.parse::<f64>().ok().map(|v| v as u64))
}

// ── Blocking HTTP (no new deps — reuse reqwest async via a mini-runtime) ──────

fn blocking_get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    let bytes = blocking_get_bytes_inner(url, true)?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

fn blocking_get_bytes(url: &str) -> Result<Vec<u8>, String> {
    blocking_get_bytes_inner(url, false)
}

fn blocking_get_bytes_inner(url: &str, github_accept: bool) -> Result<Vec<u8>, String> {
    use std::sync::OnceLock;
    // We are called from spawn_blocking, so there is already a multi-thread
    // runtime above us.  `block_on` on the current thread is the right call.
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let client = reqwest::Client::builder()
                .user_agent("nextvpn/1.0")
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|e| e.to_string())?;

            let mut req = client.get(url);
            if github_accept {
                req = req.header("Accept", "application/vnd.github+json");
            }
            let resp = req.send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {} for {url}", resp.status()));
            }
            resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
        })
    })
}
