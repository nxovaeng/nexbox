use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use serde::Deserialize;

#[derive(Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

// ── Aether ────────────────────────────────────────────────────────────────────

pub async fn download_aether(target_dir: &Path) -> Result<PathBuf, String> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let os_str = match os {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => return Err(format!("Unsupported OS: {}", os)),
    };

    let arch_str = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "arm64",
        "arm" => "armv7",
        _ => return Err(format!("Unsupported CPU architecture: {}", arch)),
    };

    let target_str = format!("aether-{}-{}", os_str, arch_str);

    let client = reqwest::Client::builder()
        .user_agent("nextvpn")
        .build()
        .map_err(|e| e.to_string())?;

    let url = "https://api.github.com/repos/CluvexStudio/Aether/releases/latest";
    let release: GithubRelease = client.get(url).send().await.map_err(|e| e.to_string())?
        .json().await.map_err(|e| e.to_string())?;

    let mut asset_url = None;
    
    for asset in release.assets {
        if asset.name.starts_with(&target_str) {
            if asset.name.ends_with(".sha256") {
                continue;
            }
            if asset.name.ends_with(".tar.gz") {
                asset_url = Some(asset.browser_download_url.clone());
                if asset.name.contains("musl") {
                    break;
                }
            } else if asset.name.ends_with(".zip") {
                return Err("ZIP archives are not supported by the auto-downloader yet.".into());
            }
        }
    }

    let download_url = asset_url.ok_or_else(|| format!("No suitable Aether core found for {}-{}", os, arch))?;

    let response = client.get(&download_url).send().await.map_err(|e| e.to_string())?;
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;

    fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;

    let final_path = target_dir.join(if os == "windows" { "aether.exe" } else { "aether" });
    
    extract_tar_gz_single(&bytes, &["aether", "aether.exe"], &final_path)?;
    set_executable(&final_path)?;

    Ok(final_path)
}

// ── Mihomo (chain engine) ─────────────────────────────────────────────────────

/// Pinned version — bump deliberately.
const MIHOMO_VERSION: &str = "v1.19.30";

pub async fn download_mihomo(target_dir: &Path) -> Result<PathBuf, String> {
    let asset_name = mihomo_asset_name()?;
    let url = format!(
        "https://github.com/MetaCubeX/mihomo/releases/download/{}/{}",
        MIHOMO_VERSION, asset_name
    );

    let client = reqwest::Client::builder()
        .user_agent("nextvpn")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;

    fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;

    let exe = if env::consts::OS == "windows" { "mihomo.exe" } else { "mihomo" };
    let final_path = target_dir.join(exe);

    if asset_name.ends_with(".gz") && !asset_name.ends_with(".tar.gz") {
        // Single-file gzip (Linux/macOS)
        use flate2::read::GzDecoder;
        use std::io::{Read, Cursor};
        let mut decoder = GzDecoder::new(Cursor::new(&bytes));
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).map_err(|e| format!("gunzip failed: {e}"))?;
        fs::write(&final_path, out).map_err(|e| format!("write failed: {e}"))?;
    } else if asset_name.ends_with(".zip") {
        // Windows zip — extract the .exe
        extract_zip_single(&bytes, &["mihomo.exe", "mihomo-windows-amd64.exe"], &final_path)?;
    } else {
        return Err(format!("Unknown archive format: {asset_name}"));
    }

    set_executable(&final_path)?;
    Ok(final_path)
}

fn mihomo_asset_name() -> Result<String, String> {
    let name = match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64")  => format!("mihomo-linux-amd64-compatible-{}.gz", MIHOMO_VERSION),
        ("linux", "aarch64") => format!("mihomo-linux-arm64-{}.gz", MIHOMO_VERSION),
        ("macos", "x86_64")  => format!("mihomo-darwin-amd64-compatible-{}.gz", MIHOMO_VERSION),
        ("macos", "aarch64") => format!("mihomo-darwin-arm64-{}.gz", MIHOMO_VERSION),
        ("windows", "x86_64") => format!("mihomo-windows-amd64-compatible-{}.zip", MIHOMO_VERSION),
        ("windows", "aarch64") => format!("mihomo-windows-arm64-{}.zip", MIHOMO_VERSION),
        (os, arch) => return Err(format!("No mihomo build for {os}/{arch}")),
    };
    Ok(name)
}

// ── Tor Expert Bundle ─────────────────────────────────────────────────────────

/// Pinned version — bump deliberately. Tor removes old versions from their CDN.
const TOR_VERSION: &str = "15.0.22";

pub async fn download_tor(target_dir: &Path) -> Result<PathBuf, String> {
    let asset_name = tor_asset_name()?;
    let url = format!(
        "https://dist.torproject.org/torbrowser/{}/{}",
        TOR_VERSION, asset_name
    );

    let client = reqwest::Client::builder()
        .user_agent("nextvpn")
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;

    // Tor expert bundle extracts to a directory structure:
    //   tor/tor (or tor.exe)
    //   tor/pluggable_transports/lyrebird (or lyrebird.exe)
    //   tor/data/geoip
    //   tor/data/geoip6
    let tor_dir = if target_dir.file_name().and_then(|s| s.to_str()) == Some("tor") {
        target_dir.to_path_buf()
    } else {
        target_dir.join("tor")
    };
    fs::create_dir_all(&tor_dir).map_err(|e| e.to_string())?;

    let exe_ext = if env::consts::OS == "windows" { ".exe" } else { "" };

    use flate2::read::GzDecoder;
    use tar::Archive;
    use std::io::Cursor;

    let tar = GzDecoder::new(Cursor::new(&bytes));
    let mut archive = Archive::new(tar);

    let wanted: &[(&str, &str)] = &[
        (&format!("tor{exe_ext}"), ""),
        (&format!("lyrebird{exe_ext}"), ""),
        ("geoip", ""),
        ("geoip6", ""),
        ("pt_config.json", ""),
        ("torrc-defaults", ""),
    ];

    for file in archive.entries().map_err(|e| e.to_string())? {
        let mut file = file.map_err(|e| e.to_string())?;
        // Ignore non-regular files (e.g. directory entries named "tor" or "data")
        if !file.header().entry_type().is_file() {
            continue;
        }

        let path = file.path().map_err(|e| e.to_string())?;
        let path_str = path.to_string_lossy();
        if path_str.starts_with("debug") || path_str.starts_with("docs") {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

        let is_wanted = wanted.iter().any(|(w, _)| name == *w)
            || (name.starts_with("lib") && (name.contains(".so") || name.ends_with(".dll")))
            || name.ends_with(".dylib")
            || name.ends_with(".json");

        if is_wanted {
            let dest = tor_dir.join(&name);
            if dest.is_dir() {
                let _ = fs::remove_dir_all(&dest);
            } else if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            file.unpack(&dest).map_err(|e| format!("unpack {name}: {e}"))?;
            set_executable(&dest).ok();
        }
    }

    // Verify the main tor binary was extracted
    let tor_bin = tor_dir.join(format!("tor{exe_ext}"));
    if !tor_bin.exists() {
        return Err("tor binary not found in expert bundle".into());
    }

    Ok(tor_bin)
}

fn tor_asset_name() -> Result<String, String> {
    let name = match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64")  => format!("tor-expert-bundle-linux-x86_64-{}.tar.gz", TOR_VERSION),
        ("macos", "x86_64")  => format!("tor-expert-bundle-macos-x86_64-{}.tar.gz", TOR_VERSION),
        ("macos", "aarch64") => format!("tor-expert-bundle-macos-aarch64-{}.tar.gz", TOR_VERSION),
        ("windows", "x86_64") => format!("tor-expert-bundle-windows-x86_64-{}.tar.gz", TOR_VERSION),
        (os, arch) => return Err(format!("No Tor expert bundle for {os}/{arch}. Tor Project does not publish one for this platform.")),
    };
    Ok(name)
}

// ── Wireproxy ─────────────────────────────────────────────────────────────────

const WIREPROXY_VERSION: &str = "v1.1.3";

pub async fn download_wireproxy(target_dir: &Path) -> Result<PathBuf, String> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let os_str = match os {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        _ => return Err(format!("Unsupported OS: {}", os)),
    };

    let arch_str = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        _ => return Err(format!("Unsupported CPU architecture: {}", arch)),
    };

    let asset_name = format!("wireproxy_{}_{}.tar.gz", os_str, arch_str);
    let url = format!(
        "https://github.com/windtf/wireproxy/releases/download/{}/{}",
        WIREPROXY_VERSION, asset_name
    );

    let client = reqwest::Client::builder()
        .user_agent("nextvpn")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Wireproxy download failed: HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;

    fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;

    let exe = if os == "windows" { "wireproxy.exe" } else { "wireproxy" };
    let final_path = target_dir.join(exe);

    extract_tar_gz_single(&bytes, &["wireproxy", "wireproxy.exe"], &final_path)?;
    set_executable(&final_path)?;

    Ok(final_path)
}

// ── Upload (save user-provided binary) ────────────────────────────────────────

pub fn save_uploaded_binary(target_dir: &Path, name: &str, data: &[u8]) -> Result<PathBuf, String> {
    fs::create_dir_all(target_dir).map_err(|e| e.to_string())?;

    let exe_ext = if env::consts::OS == "windows" { ".exe" } else { "" };
    let final_path = target_dir.join(format!("{name}{exe_ext}"));

    if data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06") {
        let candidate_names = [
            format!("{name}{exe_ext}"),
            format!("{name}"),
            format!("{name}-linux-amd64"),
            format!("{name}-linux-arm64"),
            format!("{name}-windows-amd64.exe"),
            format!("{name}-windows-arm64.exe"),
            "psiphon-tunnel-core".to_string(),
            "psiphon-tunnel-core.exe".to_string(),
            "mihomo".to_string(),
            "mihomo.exe".to_string(),
            "tor".to_string(),
            "tor.exe".to_string(),
            "wireproxy".to_string(),
            "wireproxy.exe".to_string(),
        ];
        let str_candidates: Vec<&str> = candidate_names.iter().map(|s| s.as_str()).collect();
        extract_zip_single(data, &str_candidates, &final_path)?;
    } else if data.len() > 2 && data[0] == 0x1f && data[1] == 0x8b {
        use flate2::read::GzDecoder;
        use std::io::{Read, Cursor};

        // Try tar.gz extraction first
        let is_tar = {
            let gz = GzDecoder::new(Cursor::new(data));
            let mut tar = tar::Archive::new(gz);
            match tar.entries() {
                Ok(entries) => entries.filter_map(Result::ok).any(|e| e.header().entry_type().is_file()),
                Err(_) => false,
            }
        };

        if is_tar {
            let candidate_names = [
                format!("{name}{exe_ext}"),
                format!("{name}"),
                format!("{name}-linux-amd64"),
                format!("{name}-linux-arm64"),
                "psiphon-tunnel-core".to_string(),
                "psiphon-tunnel-core-linux-x64".to_string(),
                "mihomo".to_string(),
                "tor".to_string(),
                "wireproxy".to_string(),
            ];
            let str_candidates: Vec<&str> = candidate_names.iter().map(|s| s.as_str()).collect();
            extract_tar_gz_single(data, &str_candidates, &final_path)?;
        } else {
            // Single file gzip (e.g. mihomo-linux-amd64.gz)
            let mut gz = GzDecoder::new(Cursor::new(data));
            let mut out = Vec::new();
            gz.read_to_end(&mut out).map_err(|e| format!("gunzip failed: {e}"))?;
            if final_path.is_dir() {
                let _ = fs::remove_dir_all(&final_path);
            } else if final_path.exists() {
                let _ = fs::remove_file(&final_path);
            }
            fs::write(&final_path, out).map_err(|e| format!("cannot write {name}: {e}"))?;
        }
    } else {
        if final_path.is_dir() {
            let _ = fs::remove_dir_all(&final_path);
        } else if final_path.exists() {
            let _ = fs::remove_file(&final_path);
        }
        fs::write(&final_path, data).map_err(|e| format!("cannot write {name}: {e}"))?;
    }

    set_executable(&final_path)?;

    // If saving psiphon, guarantee both psiphon and psiphon-tunnel-core exist
    if name == "psiphon" || name == "psiphon-tunnel-core" {
        let alt = if name == "psiphon" { "psiphon-tunnel-core" } else { "psiphon" };
        let alt_path = target_dir.join(format!("{alt}{exe_ext}"));
        if alt_path.is_dir() {
            let _ = fs::remove_dir_all(&alt_path);
        } else if alt_path.exists() {
            let _ = fs::remove_file(&alt_path);
        }
        let _ = fs::copy(&final_path, &alt_path);
        let _ = set_executable(&alt_path);
    }

    Ok(final_path)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn set_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn extract_tar_gz_single(bytes: &[u8], names: &[&str], dest: &Path) -> Result<(), String> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    use std::io::Cursor;

    let tar = GzDecoder::new(Cursor::new(bytes));
    let mut archive = Archive::new(tar);

    for file in archive.entries().map_err(|e| e.to_string())? {
        let mut file = file.map_err(|e| e.to_string())?;
        if !file.header().entry_type().is_file() {
            continue;
        }
        let path = file.path().map_err(|e| e.to_string())?;
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        if names.iter().any(|n| *n == file_name.as_ref()) {
            if dest.is_dir() {
                let _ = fs::remove_dir_all(dest);
            } else if dest.exists() {
                let _ = fs::remove_file(dest);
            }
            file.unpack(dest).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }

    Err(format!("binary not found in archive (looking for {:?})", names))
}

fn extract_zip_single(bytes: &[u8], names: &[&str], dest: &Path) -> Result<(), String> {
    use std::io::{Cursor, Read};

    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| format!("zip open: {e}"))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("zip entry: {e}"))?;
        let name = file.name().rsplit('/').next().unwrap_or(file.name());
        if names.iter().any(|n| *n == name) {
            let mut out = Vec::new();
            file.read_to_end(&mut out).map_err(|e| format!("zip read: {e}"))?;
            fs::write(dest, out).map_err(|e| format!("write: {e}"))?;
            return Ok(());
        }
    }

    Err(format!("binary not found in zip (looking for {:?})", names))
}

// ── Legacy alias ──────────────────────────────────────────────────────────────

/// Backwards-compatible alias used by existing call sites.
pub async fn download_core(target_dir: &Path) -> Result<PathBuf, String> {
    download_aether(target_dir).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_uploaded_binary_creates_psiphon_and_alias() {
        let temp = std::env::temp_dir().join(format!("nextvpn_test_upload_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);

        let data = b"\x7fELFfake_psiphon_binary_content_here";
        let res = save_uploaded_binary(&temp, "psiphon", data);
        assert!(res.is_ok());

        let primary = temp.join(if cfg!(windows) { "psiphon.exe" } else { "psiphon" });
        let alias = temp.join(if cfg!(windows) { "psiphon-tunnel-core.exe" } else { "psiphon-tunnel-core" });
        assert!(primary.exists(), "primary binary must exist");
        assert!(alias.exists(), "alias binary must exist");

        let _ = fs::remove_dir_all(&temp);
    }
}
