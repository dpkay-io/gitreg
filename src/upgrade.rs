use crate::error::{GitregError, Result};
use sha2::{Digest, Sha256};
use std::io::Read;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const TARGET: &str = "x86_64-unknown-linux-musl";
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const TARGET: &str = "aarch64-unknown-linux-musl";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const TARGET: &str = "x86_64-apple-darwin";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const TARGET: &str = "aarch64-apple-darwin";
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const TARGET: &str = "x86_64-pc-windows-msvc";
#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64"),
)))]
const TARGET: &str = "";

#[cfg(windows)]
const EXT: &str = "zip";
#[cfg(not(windows))]
const EXT: &str = "tar.gz";

#[cfg(windows)]
const BINARY_NAME: &str = "gitreg.exe";
#[cfg(not(windows))]
const BINARY_NAME: &str = "gitreg";

const API_URL: &str = "https://api.github.com/repos/dpkay-io/gitreg/releases/latest";
const DOWNLOAD_BASE: &str = "https://github.com/dpkay-io/gitreg/releases/latest/download";
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let s = s.split('-').next().unwrap_or(s);
    let mut parts = s.splitn(3, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

fn extract_tag_name(body: &str) -> Option<String> {
    let key = "\"tag_name\"";
    let pos = body.find(key)?;
    let rest = &body[pos + key.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn fetch_latest_tag() -> Result<String> {
    let version = env!("CARGO_PKG_VERSION");
    let response = ureq::get(API_URL)
        .set("User-Agent", &format!("gitreg/{version}"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| GitregError::Network(e.to_string()))?;
    let body = response
        .into_string()
        .map_err(|e| GitregError::Network(e.to_string()))?;
    extract_tag_name(&body).ok_or_else(|| {
        GitregError::Upgrade("could not parse tag_name from GitHub API response".into())
    })
}

fn download_archive(target: &str, ext: &str) -> Result<Vec<u8>> {
    let url = format!("{DOWNLOAD_BASE}/gitreg-latest-{target}.{ext}");
    println!("Downloading {url} ...");
    let response = ureq::get(&url)
        .set(
            "User-Agent",
            &format!("gitreg/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| GitregError::Network(e.to_string()))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_DOWNLOAD_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| GitregError::Network(e.to_string()))?;
    Ok(bytes)
}

#[cfg(not(windows))]
fn extract_binary(archive_bytes: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let gz = GzDecoder::new(archive_bytes);
    let mut archive = Archive::new(gz);
    for entry in archive
        .entries()
        .map_err(|e| GitregError::Upgrade(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| GitregError::Upgrade(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| GitregError::Upgrade(e.to_string()))?;
        if path.file_name().map(|n| n == BINARY_NAME).unwrap_or(false) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| GitregError::Upgrade(e.to_string()))?;
            return Ok(buf);
        }
    }
    Err(GitregError::Upgrade(format!(
        "binary '{BINARY_NAME}' not found in archive"
    )))
}

#[cfg(windows)]
fn extract_binary(archive_bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let cursor = Cursor::new(archive_bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|e| GitregError::Upgrade(e.to_string()))?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| GitregError::Upgrade(e.to_string()))?;
        if file.name().ends_with(BINARY_NAME) {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| GitregError::Upgrade(e.to_string()))?;
            return Ok(buf);
        }
    }
    Err(GitregError::Upgrade(format!(
        "binary '{BINARY_NAME}' not found in archive"
    )))
}

#[cfg(not(windows))]
fn self_replace(new_bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let exe = std::env::current_exe().map_err(GitregError::ExePath)?;
    let dir = exe
        .parent()
        .ok_or_else(|| GitregError::Upgrade("executable has no parent directory".into()))?;
    let tmp = dir.join(".gitreg.tmp");
    std::fs::write(&tmp, new_bytes)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&tmp, &exe)?;
    Ok(())
}

#[cfg(windows)]
fn self_replace(new_bytes: &[u8]) -> Result<()> {
    let exe = std::env::current_exe().map_err(GitregError::ExePath)?;
    let old = exe.with_extension("exe.old");
    std::fs::rename(&exe, &old)?;
    if let Err(e) = std::fs::write(&exe, new_bytes) {
        let _ = std::fs::rename(&old, &exe);
        return Err(GitregError::Upgrade(format!(
            "failed to write new binary: {e}"
        )));
    }
    Ok(())
}

fn download_sha256_sidecar(target: &str, ext: &str) -> Result<[u8; 32]> {
    let url = format!("{DOWNLOAD_BASE}/gitreg-latest-{target}.{ext}.sha256");
    let response = ureq::get(&url)
        .set(
            "User-Agent",
            &format!("gitreg/{}", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| GitregError::Network(e.to_string()))?;
    let text = response
        .into_string()
        .map_err(|e| GitregError::Network(e.to_string()))?;
    let text = text.trim_start_matches('\u{FEFF}'); // strip BOM if present
    let hex = text
        .split_whitespace()
        .next()
        .ok_or_else(|| GitregError::Upgrade("sha256 sidecar file is empty".into()))?;
    if hex.len() != 64 {
        return Err(GitregError::Upgrade(format!(
            "unexpected sha256 format ({} chars, want 64)",
            hex.len()
        )));
    }
    let mut hash = [0u8; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| GitregError::Upgrade("invalid hex in sha256 sidecar".into()))?;
    }
    Ok(hash)
}

fn verify_archive_sha256(bytes: &[u8], expected: &[u8; 32]) -> Result<()> {
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    if actual != *expected {
        return Err(GitregError::Upgrade(
            "SHA256 mismatch — archive may be corrupted or tampered with".into(),
        ));
    }
    Ok(())
}

pub fn run() -> Result<()> {
    if TARGET.is_empty() {
        return Err(GitregError::Upgrade(
            "unsupported platform — no pre-built binary available".into(),
        ));
    }

    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: v{current_version}");
    print!("Checking for updates... ");

    let latest_tag = fetch_latest_tag()?;
    println!("{latest_tag}");

    let current = parse_version(current_version)
        .ok_or_else(|| GitregError::Upgrade("could not parse current version".into()))?;
    let latest = parse_version(&latest_tag)
        .ok_or_else(|| GitregError::Upgrade(format!("could not parse latest tag: {latest_tag}")))?;

    if current >= latest {
        println!("Already up to date (v{current_version}).");
        return Ok(());
    }

    println!("Upgrading to {latest_tag} ...");
    let archive = download_archive(TARGET, EXT)?;
    print!("Verifying SHA256... ");
    let expected_hash = download_sha256_sidecar(TARGET, EXT)?;
    verify_archive_sha256(&archive, &expected_hash)?;
    println!("OK");
    let binary = extract_binary(&archive)?;
    self_replace(&binary)?;
    println!("Upgraded to {latest_tag}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_v_prefix() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
    }

    #[test]
    fn parse_version_no_prefix() {
        assert_eq!(parse_version("0.2.0"), Some((0, 2, 0)));
    }

    #[test]
    fn parse_version_with_suffix() {
        assert_eq!(parse_version("v1.0.0-rc.1"), Some((1, 0, 0)));
    }

    #[test]
    fn extract_tag_compact() {
        let json = r#"{"tag_name":"v0.2.0","name":"v0.2.0"}"#;
        assert_eq!(extract_tag_name(json), Some("v0.2.0".into()));
    }

    #[test]
    fn extract_tag_spaced() {
        let json = r#"{ "tag_name" : "v1.0.0" }"#;
        assert_eq!(extract_tag_name(json), Some("v1.0.0".into()));
    }

    #[test]
    fn extract_tag_missing() {
        let json = r#"{"name":"v1.0.0"}"#;
        assert_eq!(extract_tag_name(json), None);
    }
}
