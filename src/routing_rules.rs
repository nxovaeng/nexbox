//! Generic Routing Rules Management for Aether & Mihomo.
//!
//! Manages custom uploaded Direct (Allow/Bypass) and Block rules.
//! Compliant with Aether's routing rule specifications:
//! - [block]: Never send / refuse connection outright (domains, keyword, regex, ports, IPs)
//! - [direct]: Bypass tunnel / send out of real interface (domains, IPs, private, etc.)

use std::io::Write;
use std::path::{Path, PathBuf};

pub const DIRECT_LIST_FILENAME: &str = "direct.txt";
pub const BLOCK_LIST_FILENAME: &str = "block.txt";
pub const GENERATED_ROUTES_FILENAME: &str = "generated-routes.txt";

pub fn routing_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("routing")
}

pub fn direct_list_path(data_dir: &Path) -> PathBuf {
    routing_dir(data_dir).join(DIRECT_LIST_FILENAME)
}

pub fn block_list_path(data_dir: &Path) -> PathBuf {
    routing_dir(data_dir).join(BLOCK_LIST_FILENAME)
}

pub fn generated_routes_path(data_dir: &Path) -> PathBuf {
    routing_dir(data_dir).join(GENERATED_ROUTES_FILENAME)
}

/// Counts valid rule lines (ignoring empty lines, comments with # or //, and section headers).
pub fn count_rules(content: &str) -> usize {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("//") && !l.starts_with('['))
        .count()
}

pub fn read_direct_list(data_dir: &Path) -> String {
    let path = direct_list_path(data_dir);
    if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    }
}

pub fn read_block_list(data_dir: &Path) -> String {
    let path = block_list_path(data_dir);
    if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    }
}

pub fn count_direct_rules(data_dir: &Path) -> usize {
    count_rules(&read_direct_list(data_dir))
}

pub fn count_block_rules(data_dir: &Path) -> usize {
    count_rules(&read_block_list(data_dir))
}

pub fn has_custom_rules(data_dir: &Path) -> bool {
    let d = direct_list_path(data_dir);
    let b = block_list_path(data_dir);
    (d.exists() && count_rules(&read_direct_list(data_dir)) > 0)
        || (b.exists() && count_rules(&read_block_list(data_dir)) > 0)
}

pub fn save_direct_list(data_dir: &Path, content: &str) -> Result<usize, String> {
    let r_dir = routing_dir(data_dir);
    std::fs::create_dir_all(&r_dir)
        .map_err(|e| format!("cannot create routing directory: {e}"))?;
    let path = direct_list_path(data_dir);
    std::fs::write(&path, content)
        .map_err(|e| format!("cannot write direct rules file: {e}"))?;
    Ok(count_rules(content))
}

pub fn save_block_list(data_dir: &Path, content: &str) -> Result<usize, String> {
    let r_dir = routing_dir(data_dir);
    std::fs::create_dir_all(&r_dir)
        .map_err(|e| format!("cannot create routing directory: {e}"))?;
    let path = block_list_path(data_dir);
    std::fs::write(&path, content)
        .map_err(|e| format!("cannot write block rules file: {e}"))?;
    Ok(count_rules(content))
}

pub fn clear_direct_list(data_dir: &Path) -> Result<(), String> {
    let path = direct_list_path(data_dir);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("cannot remove direct rules file: {e}"))?;
    }
    Ok(())
}

pub fn clear_block_list(data_dir: &Path) -> Result<(), String> {
    let path = block_list_path(data_dir);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("cannot remove block rules file: {e}"))?;
    }
    Ok(())
}

/// Parses an Aether routes file (with `[block]` and `[direct]`), or if plain lines, saves as direct.
pub fn save_combined_routes(data_dir: &Path, content: &str) -> Result<(usize, usize), String> {
    let (block_content, direct_content) = split_sections(content);
    let mut block_count = 0;
    let mut direct_count = 0;

    if !block_content.is_empty() {
        block_count = save_block_list(data_dir, &block_content)?;
    }
    if !direct_content.is_empty() {
        direct_count = save_direct_list(data_dir, &direct_content)?;
    }

    // If no section headers were found, treat the whole content as direct list
    if block_count == 0 && direct_count == 0 && !content.trim().is_empty() {
        direct_count = save_direct_list(data_dir, content)?;
    }

    Ok((block_count, direct_count))
}

/// The raw IP-CIDR list for mihomo rule-providers (ipcidr format).
pub fn ip_ranges_for_mihomo(data_dir: &Path) -> String {
    let direct = read_direct_list(data_dir);
    let mut ips = Vec::new();
    for line in direct.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.contains('/') || trimmed.parse::<std::net::IpAddr>().is_ok() {
            ips.push(trimmed);
        }
    }
    if ips.is_empty() {
        "# no direct ip ranges\n127.0.0.1/32\n".to_string()
    } else {
        ips.join("\n") + "\n"
    }
}

/// The domain list for mihomo rule-providers (domain format, `+.example.com`).
pub fn domains_for_mihomo(data_dir: &Path) -> String {
    let direct = read_direct_list(data_dir);
    let mut domains = Vec::new();
    for line in direct.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.contains('/') || trimmed.starts_with("port:") || trimmed == "private" || trimmed.parse::<std::net::IpAddr>().is_ok() {
            continue;
        }
        let clean = trimmed.strip_prefix("+.").unwrap_or(trimmed);
        let clean = clean.strip_prefix("*.").unwrap_or(clean);
        let clean = clean.strip_prefix('.').unwrap_or(clean);
        if !clean.is_empty() {
            domains.push(format!("+.{clean}"));
        }
    }
    if domains.is_empty() {
        "# no direct domains\n+.localhost\n".to_string()
    } else {
        domains.join("\n") + "\n"
    }
}

/// Writes a unified `--routes` file that Aether can load:
/// Combines:
/// - Uploaded `block.txt` + `inline_block` + user_routes_file `[block]`
/// - Uploaded `direct.txt` + `inline_direct` + user_routes_file `[direct]`
pub fn write_combined_routes_file(
    data_dir: &Path,
    destination: &Path,
    user_routes_file: Option<&Path>,
    inline_block: Option<&str>,
    inline_direct: Option<&str>,
) -> Result<(), String> {
    let (user_block, user_direct) = match user_routes_file {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|error| {
                format!("cannot read the routes file {}: {error}", path.display())
            })?;
            split_sections(&text)
        }
        None => (String::new(), String::new()),
    };

    let uploaded_block = read_block_list(data_dir);
    let uploaded_direct = read_direct_list(data_dir);

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot prepare the routing directory: {error}"))?;
    }
    let mut file = std::fs::File::create(destination)
        .map_err(|error| format!("cannot write {}: {error}", destination.display()))?;

    writeln!(file, "# Generated by nextvpn routing manager")
        .and_then(|()| writeln!(file, "# Compliant with Aether routing rules"))
        .and_then(|()| writeln!(file, "[block]"))
        .map_err(|e| format!("cannot write block header: {e}"))?;

    if !uploaded_block.trim().is_empty() {
        writeln!(file, "{}", uploaded_block.trim())
            .map_err(|e| format!("cannot write block rules: {e}"))?;
    }
    if let Some(ib) = inline_block {
        if !ib.trim().is_empty() {
            writeln!(file, "{}", ib.trim())
                .map_err(|e| format!("cannot write inline block rules: {e}"))?;
        }
    }
    if !user_block.trim().is_empty() {
        writeln!(file, "{}", user_block.trim())
            .map_err(|e| format!("cannot write user block rules: {e}"))?;
    }

    writeln!(file, "\n[direct]")
        .map_err(|e| format!("cannot write direct header: {e}"))?;

    if !uploaded_direct.trim().is_empty() {
        writeln!(file, "{}", uploaded_direct.trim())
            .map_err(|e| format!("cannot write direct rules: {e}"))?;
    }
    if let Some(id) = inline_direct {
        if !id.trim().is_empty() {
            writeln!(file, "{}", id.trim())
                .map_err(|e| format!("cannot write inline direct rules: {e}"))?;
        }
    }
    if !user_direct.trim().is_empty() {
        writeln!(file, "{}", user_direct.trim())
            .map_err(|e| format!("cannot write user direct rules: {e}"))?;
    }

    Ok(())
}

/// Splits an Aether-compatible routes configuration into (block, direct) sections.
pub fn split_sections(text: &str) -> (String, String) {
    let mut block = String::new();
    let mut direct = String::new();
    let mut current: Option<&mut String> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        let lowered = trimmed.to_lowercase();
        if lowered == "[block]" {
            current = Some(&mut block);
            continue;
        }
        if lowered == "[direct]" {
            current = Some(&mut direct);
            continue;
        }
        if lowered.starts_with('[') {
            current = None;
            continue;
        }
        if let Some(target) = current.as_deref_mut() {
            target.push_str(trimmed);
            target.push('\n');
        }
    }
    (block, direct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_sections_parses_block_and_direct() {
        let sample = r#"
# Sample Aether routes file
[block]
ads.example.com
port:25
keyword:doubleclick

[direct]
private
bank.ir
10.0.0.0/8
"#;
        let (block, direct) = split_sections(sample);
        assert!(block.contains("ads.example.com"));
        assert!(block.contains("port:25"));
        assert!(block.contains("keyword:doubleclick"));
        assert!(direct.contains("private"));
        assert!(direct.contains("bank.ir"));
        assert!(direct.contains("10.0.0.0/8"));
    }

    #[test]
    fn custom_lists_save_and_count_properly() {
        let temp_dir = std::env::temp_dir().join(format!("routing_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);

        let direct_input = "domain1.com\n192.168.1.0/24\n# Comment\n\nprivate\n";
        let count = save_direct_list(&temp_dir, direct_input).unwrap();
        assert_eq!(count, 3);
        assert_eq!(count_direct_rules(&temp_dir), 3);

        let block_input = "malware.com\nport:25\n";
        let b_count = save_block_list(&temp_dir, block_input).unwrap();
        assert_eq!(b_count, 2);
        assert_eq!(count_block_rules(&temp_dir), 2);

        let out = temp_dir.join("combined.txt");
        write_combined_routes_file(&temp_dir, &out, None, Some("extra-block.com"), Some("extra-direct.com")).unwrap();
        let combined = std::fs::read_to_string(&out).unwrap();

        assert!(combined.contains("[block]"));
        assert!(combined.contains("malware.com"));
        assert!(combined.contains("extra-block.com"));
        assert!(combined.contains("[direct]"));
        assert!(combined.contains("domain1.com"));
        assert!(combined.contains("private"));
        assert!(combined.contains("extra-direct.com"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
