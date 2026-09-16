use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use crate::app_context::AppContext;
use super::types::CoreItem;
use super::process::{core_version, hide_console_window, resolve_core_path};

pub fn find_binary_in_candidates(app: &AppContext, core_name: &str, binary_name: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(data_dir) = app.path().app_data_dir() {
        candidates.push(data_dir.join(core_name).join(binary_name));
        candidates.push(data_dir.join("bin").join(core_name).join(binary_name));
        candidates.push(data_dir.join("bin").join(binary_name));
        candidates.push(data_dir.join(binary_name));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("data").join(core_name).join(binary_name));
        candidates.push(cwd.join("nextvpn").join("data").join(core_name).join(binary_name));
        candidates.push(cwd.join("data").join("bin").join(core_name).join(binary_name));
        candidates.push(cwd.join("nextvpn").join("data").join("bin").join(core_name).join(binary_name));
        candidates.push(cwd.join("data").join("bin").join(binary_name));
        candidates.push(cwd.join("nextvpn").join("data").join("bin").join(binary_name));
        candidates.push(cwd.join("data").join(binary_name));
        candidates.push(cwd.join("nextvpn").join("data").join(binary_name));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("data").join(core_name).join(binary_name));
            candidates.push(parent.join(core_name).join(binary_name));
            candidates.push(parent.join(binary_name));
            if let Some(gp) = parent.parent() {
                candidates.push(gp.join("data").join(core_name).join(binary_name));
                candidates.push(gp.join("nextvpn").join("data").join(core_name).join(binary_name));
            }
        }
    }
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join(core_name).join(binary_name));
        candidates.push(resources.join(binary_name));
        candidates.push(resources.join("binaries").join(binary_name));
    }
    for candidate in candidates {
        if candidate.is_file() {
            return candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    None
}

pub fn probe_binary_version(cmd: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("RUST_LOG");
    hide_console_window(&mut command);
    let child = command.spawn().ok()?;
    let output = child.wait_with_output().ok()?;
    let text = if !output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::from_utf8_lossy(&output.stderr).to_string()
    };
    let first_line = text.lines().next()?.trim().to_string();
    if first_line.is_empty() {
        return None;
    }
    if let Some(part) = first_line.split_whitespace().find(|w| w.starts_with('v') || w.starts_with("15.") || w.starts_with("0.")) {
        Some(part.to_string())
    } else {
        Some(first_line)
    }
}

pub fn core_inventory(app: &AppContext) -> Vec<CoreItem> {
    let mut inventory = Vec::new();
    
    // Check Aether
    let aether_bin = if cfg!(windows) { "aether.exe" } else { "aether" };
    let aether_path = resolve_core_path(app, None)
        .ok()
        .or_else(|| find_binary_in_candidates(app, "aether", aether_bin));
    let (aether_installed, aether_version, aether_path_str) = match aether_path {
        Some(ref p) => {
            let ver = core_version(p).ok().or_else(|| probe_binary_version(p, &["--version"])).or(Some("v1.9.0".to_string()));
            (true, ver, Some(p.to_string_lossy().into_owned()))
        },
        None => (false, None, None),
    };
    inventory.push(CoreItem {
        name: "aether".to_string(),
        installed: aether_installed,
        version: aether_version,
        path: aether_path_str,
    });
    
    // Check Mihomo
    let mihomo_bin = if cfg!(windows) { "mihomo.exe" } else { "mihomo" };
    let mihomo_path = crate::chain::locate(app)
        .ok()
        .or_else(|| find_binary_in_candidates(app, "mihomo", mihomo_bin));
    let (mihomo_installed, mihomo_version, mihomo_path_str) = match mihomo_path {
        Some(ref p) => {
            let ver = probe_binary_version(p, &["-v"]).or(Some("v1.19.21".to_string()));
            (true, ver, Some(p.to_string_lossy().into_owned()))
        },
        None => (false, None, None),
    };
    inventory.push(CoreItem {
        name: "mihomo".to_string(),
        installed: mihomo_installed,
        version: mihomo_version,
        path: mihomo_path_str,
    });

    // Check Wireproxy
    let wireproxy_bin = if cfg!(windows) { "wireproxy.exe" } else { "wireproxy" };
    let wireproxy_path = find_binary_in_candidates(app, "wireproxy", wireproxy_bin);
    let (wireproxy_installed, wireproxy_version, wireproxy_path_str) = match wireproxy_path {
        Some(ref p) => {
            let ver = probe_binary_version(p, &["--version"]).or(Some("v1.1.3".to_string()));
            (true, ver, Some(p.to_string_lossy().into_owned()))
        },
        None => (false, None, None),
    };
    inventory.push(CoreItem {
        name: "wireproxy".to_string(),
        installed: wireproxy_installed,
        version: wireproxy_version,
        path: wireproxy_path_str,
    });
    
    // Check Psiphon
    let psiphon_bin = if cfg!(windows) { "psiphon-tunnel-core.exe" } else { "psiphon-tunnel-core" };
    let psiphon_path = crate::psiphon::locate(app)
        .ok()
        .or_else(|| find_binary_in_candidates(app, "psiphon", psiphon_bin))
        .or_else(|| find_binary_in_candidates(app, "psiphon", if cfg!(windows) { "psiphon.exe" } else { "psiphon" }));
    let (psiphon_installed, psiphon_version, psiphon_path_str) = match psiphon_path {
        Some(ref p) => {
            let ver = probe_binary_version(p, &["--version"]).or(Some("v2.0.41".to_string()));
            (true, ver, Some(p.to_string_lossy().into_owned()))
        },
        None => (false, None, None),
    };
    inventory.push(CoreItem {
        name: "psiphon".to_string(),
        installed: psiphon_installed,
        version: psiphon_version,
        path: psiphon_path_str,
    });

    // Check Tor
    let tor_bin = if cfg!(windows) { "tor.exe" } else { "tor" };
    let tor_path = crate::tor::locate(app, tor_bin, &[])
        .ok()
        .or_else(|| find_binary_in_candidates(app, "tor", tor_bin));
    let (tor_installed, tor_version, tor_path_str) = match tor_path {
        Some(ref p) => {
            let ver = probe_binary_version(p, &["--version"]).or(Some("15.0.22".to_string()));
            (true, ver, Some(p.to_string_lossy().into_owned()))
        },
        None => (false, None, None),
    };
    inventory.push(CoreItem {
        name: "tor".to_string(),
        installed: tor_installed,
        version: tor_version,
        path: tor_path_str,
    });

    // Check Warpscout
    let ws_bin = if cfg!(windows) { "warpscout.exe" } else { "warpscout" };
    let ws_path = find_binary_in_candidates(app, "warpscout", ws_bin);
    let (ws_installed, ws_version, ws_path_str) = match ws_path {
        Some(ref p) => {
            let ver = probe_binary_version(p, &["version"]).or(Some("0.16.0".to_string()));
            (true, ver, Some(p.to_string_lossy().into_owned()))
        },
        None => (false, None, None),
    };
    inventory.push(CoreItem {
        name: "warpscout".to_string(),
        installed: ws_installed,
        version: ws_version,
        path: ws_path_str,
    });
    
    inventory
}
