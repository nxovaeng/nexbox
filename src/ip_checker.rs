use std::{
    net::SocketAddr,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use crate::socks_instance::ConnectivityResult;

const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpCheckKind {
    CloudflareTrace,
    IpSb,
    IpWhoIs,
    IfconfigCo,
    IpApiCo,
    MyIp,
    IpInfo,
}

#[derive(Debug, Clone)]
pub struct IpCheckTarget {
    pub name: &'static str,
    pub url: &'static str,
    pub kind: IpCheckKind,
}

pub static TARGET_POOL: &[IpCheckTarget] = &[
    IpCheckTarget {
        name: "IP.SB",
        url: "https://api.ip.sb/geoip",
        kind: IpCheckKind::IpSb,
    },
    IpCheckTarget {
        name: "IPWho.is",
        url: "https://ipwho.is/",
        kind: IpCheckKind::IpWhoIs,
    },
    IpCheckTarget {
        name: "Cloudflare",
        url: "https://www.cloudflare.com/cdn-cgi/trace",
        kind: IpCheckKind::CloudflareTrace,
    },
    IpCheckTarget {
        name: "ifconfig.co",
        url: "https://ifconfig.co/json",
        kind: IpCheckKind::IfconfigCo,
    },
    IpCheckTarget {
        name: "ipapi.co",
        url: "https://ipapi.co/json/",
        kind: IpCheckKind::IpApiCo,
    },
    IpCheckTarget {
        name: "MyIP",
        url: "https://api.myip.com",
        kind: IpCheckKind::MyIp,
    },
    IpCheckTarget {
        name: "ipinfo.io",
        url: "https://ipinfo.io/json",
        kind: IpCheckKind::IpInfo,
    },
];

static ROTATION_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Probes egress connectivity and IP info through the specified SOCKS5 proxy,
/// rotating between multiple providers and automatically falling back if a provider
/// hits HTTP 429 rate limit or connection error.
pub async fn check_ip_via_socks(
    socks: SocketAddr,
    auth: Option<(&str, &str)>,
) -> ConnectivityResult {
    let now_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    let proxy_url = match auth {
        Some((u, p)) => format!("socks5h://{}:{}@{}", u, p, socks),
        None => format!("socks5h://{}", socks),
    };

    let proxy = match reqwest::Proxy::all(&proxy_url) {
        Ok(p) => p,
        Err(e) => {
            return ConnectivityResult {
                success: false,
                ip: None,
                country: None,
                colo: None,
                org: None,
                latency_ms: None,
                error: Some(format!("Failed to create SOCKS5 proxy handle: {e}")),
                checked_at: now_ts,
                provider: None,
            };
        }
    };

    let client = match reqwest::Client::builder()
        .proxy(proxy)
        .timeout(PROBE_TIMEOUT)
        .user_agent("curl/8.5.0")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ConnectivityResult {
                success: false,
                ip: None,
                country: None,
                colo: None,
                org: None,
                latency_ms: None,
                error: Some(format!("Failed to build HTTP client: {e}")),
                checked_at: now_ts,
                provider: None,
            };
        }
    };

    let pool_len = TARGET_POOL.len();
    let start_idx = ROTATION_COUNTER.fetch_add(1, Ordering::SeqCst) % pool_len;
    let mut last_err = None;

    for i in 0..pool_len {
        let idx = (start_idx + i) % pool_len;
        let target = &TARGET_POOL[idx];

        let started = Instant::now();
        let resp_res = client
            .get(target.url)
            .header("Accept", "application/json, text/plain, */*")
            .send()
            .await;

        match resp_res {
            Ok(resp) => {
                let status = resp.status();
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    println!("[ip_checker] {} returned 429 Too Many Requests, falling back to next provider...", target.name);
                    last_err = Some(format!("{} returned 429 (Rate Limited)", target.name));
                    continue;
                }
                if !status.is_success() {
                    println!("[ip_checker] {} returned HTTP {}, trying next provider...", target.name, status);
                    last_err = Some(format!("{} returned HTTP {}", target.name, status));
                    continue;
                }

                let latency_ms = (started.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0;
                let text = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => {
                        last_err = Some(format!("Failed to read body from {}: {e}", target.name));
                        continue;
                    }
                };

                if let Some(parsed) = parse_target_response(target, &text, latency_ms, now_ts) {
                    return parsed;
                } else {
                    last_err = Some(format!("Could not parse response from {}", target.name));
                }
            }
            Err(e) => {
                println!("[ip_checker] Probe {} failed ({e}), trying next provider...", target.name);
                last_err = Some(format!("{}: {e}", target.name));
            }
        }
    }

    ConnectivityResult {
        success: false,
        ip: None,
        country: None,
        colo: None,
        org: None,
        latency_ms: None,
        error: last_err.or(Some("All IP check endpoints failed".to_string())),
        checked_at: now_ts,
        provider: None,
    }
}

fn parse_target_response(
    target: &IpCheckTarget,
    body: &str,
    latency_ms: f64,
    now_ts: u64,
) -> Option<ConnectivityResult> {
    match target.kind {
        IpCheckKind::CloudflareTrace => {
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
            if ip.is_some() {
                Some(ConnectivityResult {
                    success: true,
                    ip,
                    country,
                    colo,
                    org: Some("Cloudflare Edge".to_string()),
                    latency_ms: Some(latency_ms),
                    error: None,
                    checked_at: now_ts,
                    provider: Some(target.name.to_string()),
                })
            } else {
                None
            }
        }
        IpCheckKind::IpSb => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let ip = v.get("ip").and_then(|x| x.as_str()).map(|s| s.to_string())?;
            let country = v.get("country_code").and_then(|x| x.as_str()).map(|s| s.to_uppercase());
            let city = v.get("city").and_then(|x| x.as_str()).map(|s| s.to_string());
            let isp = v.get("isp").and_then(|x| x.as_str()).map(|s| s.to_string());
            let asn = v.get("asn").and_then(|x| x.as_u64());
            let org_str = match (isp, asn) {
                (Some(i), Some(a)) => Some(format!("{i} (AS{a})")),
                (Some(i), None) => Some(i),
                _ => None,
            };
            Some(ConnectivityResult {
                success: true,
                ip: Some(ip),
                country,
                colo: city,
                org: org_str,
                latency_ms: Some(latency_ms),
                error: None,
                checked_at: now_ts,
                provider: Some(target.name.to_string()),
            })
        }
        IpCheckKind::IpWhoIs => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let ip = v.get("ip").and_then(|x| x.as_str()).map(|s| s.to_string())?;
            let country = v.get("country_code").and_then(|x| x.as_str()).map(|s| s.to_uppercase());
            let city = v.get("city").and_then(|x| x.as_str()).map(|s| s.to_string());
            let isp = v.get("connection")
                .and_then(|c| c.get("isp"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            Some(ConnectivityResult {
                success: true,
                ip: Some(ip),
                country,
                colo: city,
                org: isp,
                latency_ms: Some(latency_ms),
                error: None,
                checked_at: now_ts,
                provider: Some(target.name.to_string()),
            })
        }
        IpCheckKind::IfconfigCo => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let ip = v.get("ip").and_then(|x| x.as_str()).map(|s| s.to_string())?;
            let country = v.get("country_iso").and_then(|x| x.as_str()).map(|s| s.to_uppercase());
            let city = v.get("city").and_then(|x| x.as_str()).map(|s| s.to_string());
            let org = v.get("asn_org").and_then(|x| x.as_str()).map(|s| s.to_string());
            Some(ConnectivityResult {
                success: true,
                ip: Some(ip),
                country,
                colo: city,
                org,
                latency_ms: Some(latency_ms),
                error: None,
                checked_at: now_ts,
                provider: Some(target.name.to_string()),
            })
        }
        IpCheckKind::IpApiCo => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let ip = v.get("ip").and_then(|x| x.as_str()).map(|s| s.to_string())?;
            let country = v.get("country_code").and_then(|x| x.as_str()).map(|s| s.to_uppercase());
            let city = v.get("city").and_then(|x| x.as_str()).map(|s| s.to_string());
            let org = v.get("org").and_then(|x| x.as_str()).map(|s| s.to_string());
            Some(ConnectivityResult {
                success: true,
                ip: Some(ip),
                country,
                colo: city,
                org,
                latency_ms: Some(latency_ms),
                error: None,
                checked_at: now_ts,
                provider: Some(target.name.to_string()),
            })
        }
        IpCheckKind::MyIp => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let ip = v.get("ip").and_then(|x| x.as_str()).map(|s| s.to_string())?;
            let country = v.get("cc").and_then(|x| x.as_str()).map(|s| s.to_uppercase());
            let country_name = v.get("country").and_then(|x| x.as_str()).map(|s| s.to_string());
            Some(ConnectivityResult {
                success: true,
                ip: Some(ip),
                country,
                colo: country_name,
                org: None,
                latency_ms: Some(latency_ms),
                error: None,
                checked_at: now_ts,
                provider: Some(target.name.to_string()),
            })
        }
        IpCheckKind::IpInfo => {
            let v: serde_json::Value = serde_json::from_str(body).ok()?;
            let ip = v.get("ip").and_then(|x| x.as_str()).map(|s| s.to_string())?;
            let country = v.get("country").and_then(|x| x.as_str()).map(|s| s.to_uppercase());
            let city = v.get("city").and_then(|x| x.as_str()).map(|s| s.to_string());
            let org = v.get("org").and_then(|x| x.as_str()).map(|s| s.to_string());
            Some(ConnectivityResult {
                success: true,
                ip: Some(ip),
                country,
                colo: city,
                org,
                latency_ms: Some(latency_ms),
                error: None,
                checked_at: now_ts,
                provider: Some(target.name.to_string()),
            })
        }
    }
}
