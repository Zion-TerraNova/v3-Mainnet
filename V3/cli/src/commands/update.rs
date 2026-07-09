use anyhow::{bail, Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm};
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{self, Config};
use crate::ui;

const DOWNLOADS_BASE: &str = "https://zionterranova.com/api/downloads";
const RELEASE_LINE: &str = "v2.9.9 Pure Code operator line";
const WORKSPACE_TRACK: &str = "V3 clean-room mainnet track";

#[derive(Debug, Deserialize)]
struct DownloadsIndex {
    files: Vec<RemoteFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct RemoteFile {
    name: String,
    size: u64,
    modified: String,
}

pub fn print_version_surface(cfg: &Config) -> Result<()> {
    ui::print_header("ZION CLI Version");
    ui::print_row("Binary", "zion");
    ui::print_row("Version", env!("CARGO_PKG_VERSION"));
    ui::print_row("Release", RELEASE_LINE);
    ui::print_row("Workspace", WORKSPACE_TRACK);
    ui::print_row("Node host", &cfg.node.rpc_host);
    ui::print_row("Pool host", &cfg.pool.host);
    ui::print_row("Artifact", detect_artifact_name()?);
    if let Ok(path) = config::config_path() {
        ui::print_row("Config", &path.display().to_string());
    }
    println!();
    ui::print_info("Update commands:");
    println!("  1. Check published artifact: zion update --check");
    println!("  2. Install latest artifact:  zion update --yes");
    println!();
    Ok(())
}

pub async fn run(_cfg: &Config, check: bool, yes: bool) -> Result<()> {
    run_with_auto_check(_cfg, check, yes, false).await
}

pub async fn run_with_auto_check(
    _cfg: &Config,
    check: bool,
    yes: bool,
    auto_check: bool,
) -> Result<()> {
    let artifact = detect_artifact_name()?;
    let current_exe = env::current_exe().context("Cannot resolve current zion executable")?;
    let client = Client::builder()
        .timeout(Duration::from_secs(if auto_check { 5 } else { 90 }))
        .build()
        .context("Cannot create update HTTP client")?;

    if auto_check {
        // Silent check for background use
        if let Ok(expected_sha) = fetch_expected_sha256(&client, artifact).await {
            if let Ok(current_sha) = hash_file(&current_exe) {
                if current_sha != expected_sha {
                    println!();
                    ui::print_warn(">>> A newer version of ZION CLI is available! <<<");
                    ui::print_info("Run `zion update` to upgrade to the latest version.");
                    println!();
                }
            }
        }
        return Ok(());
    }

    ui::print_header(if check {
        "ZION CLI Update Check"
    } else {
        "ZION CLI Auto Update"
    });
    ui::print_row("Binary", &current_exe.display().to_string());
    ui::print_row("Artifact", artifact);

    if let Some(remote) = fetch_remote_metadata(&client, artifact).await? {
        ui::print_row("Remote size", &format!("{} bytes", remote.size));
        ui::print_row("Remote mtime", &remote.modified);
    }

    let expected_sha = fetch_expected_sha256(&client, artifact).await?;
    let current_sha = hash_file(&current_exe)?;
    ui::print_row("Installed sha", &short_hash(&current_sha));
    ui::print_row("Latest sha", &short_hash(&expected_sha));
    println!();

    if current_sha == expected_sha {
        ui::print_ok("This zion binary already matches the latest published artifact.");
        println!();
        return Ok(());
    }

    ui::print_warn("A newer published CLI artifact is available for this platform.");
    if check {
        ui::print_info("Run `zion update --yes` to download and replace the current binary.");
        println!();
        return Ok(());
    }

    if !yes {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            bail!("Non-interactive shell detected. Re-run with `zion update --yes` to apply the update.");
        }
        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Download and replace the current zion binary?")
            .default(true)
            .interact()?;
        if !confirmed {
            ui::print_warn("Update cancelled by operator.");
            println!();
            return Ok(());
        }
    }

    let binary_bytes = download_binary(&client, artifact).await?;
    let downloaded_sha = hash_bytes(&binary_bytes);
    if downloaded_sha != expected_sha {
        bail!(
            "Downloaded binary checksum mismatch. Expected {}, got {}",
            short_hash(&expected_sha),
            short_hash(&downloaded_sha)
        );
    }

    let staged_path = write_staged_binary(&current_exe, &binary_bytes)?;

    #[cfg(target_family = "unix")]
    {
        let backup_path = replace_current_binary(&current_exe, &staged_path)?;
        ui::print_ok("CLI binary updated successfully.");
        ui::print_row("Backup", &backup_path.display().to_string());
        ui::print_warn("Restart zion to run the updated binary image.");
        println!();
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let final_stage = stage_windows_binary(&current_exe, &staged_path)?;
        ui::print_warn("Windows cannot replace the running executable in place.");
        ui::print_row("Staged", &final_stage.display().to_string());
        ui::print_info("Close the current zion process and rename the staged file over zion.exe.");
        println!();
        return Ok(());
    }

    #[allow(unreachable_code)]
    {
        ui::print_info(&format!(
            "Updated binary staged at {}",
            staged_path.display()
        ));
        println!();
        Ok(())
    }
}

fn detect_artifact_name() -> Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => Ok("zion-cli-macos-arm64"),
        ("linux", "x86_64") => Ok("zion-cli-linux-x86_64"),
        ("linux", "aarch64") => Ok("zion-cli-linux-arm64"),
        ("windows", "x86_64") => Ok("zion-cli-windows-x86_64.exe"),
        (os, arch) => bail!(
            "Unsupported auto-update platform: {} {}. Download the correct binary manually from {}/download.",
            os,
            arch,
            "https://zionterranova.com"
        ),
    }
}

async fn fetch_remote_metadata(client: &Client, artifact: &str) -> Result<Option<RemoteFile>> {
    let response = client
        .get(DOWNLOADS_BASE)
        .send()
        .await
        .context("Cannot query downloads index")?
        .error_for_status()
        .context("Downloads index returned a non-success status")?;

    let index: DownloadsIndex = response
        .json()
        .await
        .context("Downloads index returned invalid JSON")?;

    Ok(index.files.into_iter().find(|file| file.name == artifact))
}

async fn fetch_expected_sha256(client: &Client, artifact: &str) -> Result<String> {
    let sha_text = client
        .get(format!("{}/{}.sha256", DOWNLOADS_BASE, artifact))
        .send()
        .await
        .with_context(|| format!("Cannot fetch checksum for {}", artifact))?
        .error_for_status()
        .with_context(|| {
            format!(
                "Checksum endpoint returned a non-success status for {}",
                artifact
            )
        })?
        .text()
        .await
        .with_context(|| {
            format!(
                "Checksum endpoint returned unreadable content for {}",
                artifact
            )
        })?;

    parse_checksum(&sha_text)
}

async fn download_binary(client: &Client, artifact: &str) -> Result<Vec<u8>> {
    ui::print_info(&format!("Downloading latest {} artifact...", artifact));
    let bytes = client
        .get(format!("{}/{}", DOWNLOADS_BASE, artifact))
        .send()
        .await
        .with_context(|| format!("Cannot download {}", artifact))?
        .error_for_status()
        .with_context(|| format!("Download returned a non-success status for {}", artifact))?
        .bytes()
        .await
        .with_context(|| format!("Downloaded body for {} could not be read", artifact))?;
    Ok(bytes.to_vec())
}

fn parse_checksum(text: &str) -> Result<String> {
    let candidate = text
        .split_whitespace()
        .find(|part| part.len() == 64 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|part| part.to_ascii_lowercase())
        .context("Checksum file did not contain a valid SHA-256 digest")?;
    Ok(candidate)
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Cannot open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn short_hash(hash: &str) -> String {
    let end = hash.len().min(12);
    hash[..end].to_string()
}

fn write_staged_binary(current_exe: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let staged_path = sibling_path(current_exe, ".downloading")?;
    if staged_path.exists() {
        fs::remove_file(&staged_path).ok();
    }

    let mut file = fs::File::create(&staged_path)
        .with_context(|| format!("Cannot create staged binary at {}", staged_path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("Cannot write staged binary at {}", staged_path.display()))?;
    file.flush()?;

    let permissions = fs::metadata(current_exe)
        .with_context(|| format!("Cannot read permissions from {}", current_exe.display()))?
        .permissions();
    fs::set_permissions(&staged_path, permissions).with_context(|| {
        format!(
            "Cannot set executable permissions on {}",
            staged_path.display()
        )
    })?;

    Ok(staged_path)
}

#[cfg(target_family = "unix")]
fn replace_current_binary(current_exe: &Path, staged_path: &Path) -> Result<PathBuf> {
    let backup_path = sibling_path(current_exe, ".previous")?;
    if backup_path.exists() {
        fs::remove_file(&backup_path).ok();
    }

    fs::rename(current_exe, &backup_path).with_context(|| {
        format!(
            "Cannot move current binary {} to backup {}. Check write permissions.",
            current_exe.display(),
            backup_path.display()
        )
    })?;

    if let Err(err) = fs::rename(staged_path, current_exe) {
        let _ = fs::rename(&backup_path, current_exe);
        return Err(err).with_context(|| {
            format!(
                "Cannot activate updated binary at {}. Original binary was restored.",
                current_exe.display()
            )
        });
    }

    Ok(backup_path)
}

#[cfg(target_os = "windows")]
fn stage_windows_binary(current_exe: &Path, staged_path: &Path) -> Result<PathBuf> {
    let final_stage = sibling_path(current_exe, ".new")?;
    if final_stage.exists() {
        fs::remove_file(&final_stage).ok();
    }
    fs::rename(staged_path, &final_stage)
        .with_context(|| format!("Cannot stage updated binary at {}", final_stage.display()))?;
    Ok(final_stage)
}

fn sibling_path(current_exe: &Path, suffix: &str) -> Result<PathBuf> {
    let file_name = current_exe
        .file_name()
        .context("Current executable path has no file name")?;
    let mut new_name = file_name.to_os_string();
    new_name.push(suffix);
    Ok(current_exe.with_file_name(new_name))
}

#[cfg(test)]
mod tests {
    use super::{parse_checksum, short_hash, sibling_path};
    use std::path::Path;

    #[test]
    fn parse_checksum_accepts_shasum_style_line() {
        let parsed = parse_checksum(
            "d043e794be1d6954a0e701cf66dceabd41fee403ce75a2c9ea78b97ae2d6d081  zion-cli-macos-arm64\n",
        )
        .expect("checksum should parse");

        assert_eq!(
            parsed,
            "d043e794be1d6954a0e701cf66dceabd41fee403ce75a2c9ea78b97ae2d6d081"
        );
    }

    #[test]
    fn parse_checksum_rejects_invalid_content() {
        let err = parse_checksum("not-a-checksum").expect_err("invalid checksum should fail");
        assert!(err.to_string().contains("Checksum file did not contain"));
    }

    #[test]
    fn sibling_path_appends_suffix_to_filename() {
        let path = sibling_path(Path::new("/tmp/zion"), ".previous").expect("path should build");
        assert_eq!(path, Path::new("/tmp/zion.previous"));
    }

    #[test]
    fn short_hash_limits_output_width() {
        assert_eq!(short_hash("1234567890abcdef"), "1234567890ab");
    }
}
