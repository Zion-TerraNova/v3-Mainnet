//! Self-contained binary bundle — embeds or downloads the ZION node, pool,
//! and miner executables so users only need one `zion` CLI file.
//!
//! - **Windows x86_64:** Binaries are embedded directly (include_bytes!).
//! - **Linux / macOS:** Binaries are downloaded from GitHub Releases on first
//!   use and cached in `~/.zion/bin/`.
//!
//! On first use of a bundled service, the executable is extracted/downloaded to
//! `~/.zion/bin/<name>.exe` (Windows) or `~/.zion/bin/<name>` (Unix). The
//! operation only happens if the file is missing, so repeated runs are fast.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const RELEASE_TAG: &str = "v3.0.5-beta";
const GITHUB_OWNER: &str = "Zion-TerraNova";
const GITHUB_REPO: &str = "v3-Mainnet";

/// Return the path to a usable bundled binary, extracting or downloading it
/// first if needed.
///
/// Supported names: `node`, `pool`, `miner`.
/// On Windows the extracted file has `.exe` extension.
pub fn ensure_binary(name: &str) -> Result<PathBuf> {
    let dir = bundle_dir()?;
    let target = dir.join(binary_filename(name));

    if !target.exists() {
        extract_or_download(name, &target)?;
    }

    Ok(target)
}

/// Force re-extraction / re-download of all bundled binaries.
/// Useful for `zion doctor` or after a CLI update.
pub fn extract_all() -> Result<Vec<(String, PathBuf)>> {
    let names = ["node", "pool", "miner"];
    let mut out = Vec::new();
    for name in names {
        match ensure_binary(name) {
            Ok(path) => out.push((name.to_string(), path)),
            Err(e) => {
                // Don't fail completely if one binary can't be obtained.
                eprintln!("  ⚠ Could not obtain {}: {}", name, e);
            }
        }
    }
    if out.is_empty() {
        return Err(anyhow!("could not obtain any bundled binaries"));
    }
    Ok(out)
}

fn bundle_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".zion").join("bin"))
        .ok_or_else(|| anyhow!("cannot determine home directory"))
}

fn binary_filename(name: &str) -> String {
    if cfg!(windows) && !name.ends_with(".exe") {
        format!("{}.exe", name)
    } else {
        name.to_string()
    }
}

fn extract_or_download(name: &str, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    // Try embedded bytes first (Windows x86_64).
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        if let Ok(bytes) = bundled_bytes(name) {
            fs::write(target, bytes)
                .with_context(|| format!("extract {} to {}", name, target.display()))?;
            return Ok(());
        }
    }

    // No embedded binary — download from GitHub Releases.
    download_binary(name, target)
}

/// Download a binary from GitHub Releases and extract it from the tar.gz/zip.
///
/// The actual network request is performed on a dedicated std::thread so the
/// reqwest::blocking runtime is created and dropped outside the tokio async
/// context, avoiding a runtime panic when the CLI is spawned from `#[tokio::main]`.
fn download_binary(name: &str, target: &Path) -> Result<()> {
    let platform = current_platform()?;
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let asset_name = format!("zion-{}-{}.{}", name, platform, ext);
    let url = format!(
        "https://github.com/{}/{}/releases/download/{}/{}",
        GITHUB_OWNER, GITHUB_REPO, RELEASE_TAG, asset_name
    );

    eprintln!("  ◉ Downloading {} from GitHub Releases...", name);

    let name_for_thread = name.to_string();
    let asset_name_for_thread = asset_name.clone();
    let url_for_thread = url.clone();

    let body_bytes = run_off_thread(move || -> Result<Vec<u8>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;

        let resp = client
            .get(&url_for_thread)
            .header("User-Agent", "zion-cli/3.0.5")
            .send()
            .map_err(|e| anyhow!("download failed: {} ({})", e, url_for_thread))?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "GitHub returned HTTP {} for {} — the binary may not be available for this platform yet. \
                 Build from source: cargo build --release -p zion-{}",
                resp.status(),
                asset_name_for_thread,
                binary_crate_name(&name_for_thread)
            ));
        }

        let body = resp.bytes().map_err(|e| anyhow!("read download body: {}", e))?;
        Ok(body.to_vec())
    })?;

    if cfg!(windows) {
        // Extract from zip
        extract_from_zip(&body_bytes, target, name, &platform)?;
    } else {
        // Extract from tar.gz
        extract_from_tar_gz(&body_bytes, target, name, &platform)?;
    }

    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(target)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(target, perms)?;
    }

    eprintln!("  ✓ {} downloaded to {}", name, target.display());
    Ok(())
}

/// Run a blocking, `'static` closure on a dedicated std::thread. If we are
/// inside a tokio runtime, wrap the join in `block_in_place` so we don't block
/// an executor thread.
fn run_off_thread<F, R>(f: F) -> Result<R>
where
    F: FnOnce() -> Result<R> + Send + 'static,
    R: Send + 'static,
{
    let join = if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| std::thread::spawn(f).join())
    } else {
        std::thread::spawn(f).join()
    };

    join.map_err(|_| anyhow!("download thread panicked"))?
}

fn extract_from_tar_gz(data: &[u8], target: &Path, expected_name: &str, platform: &str) -> Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let decoder = GzDecoder::new(data);
    let mut archive = Archive::new(decoder);

    let bin_name = binary_filename(expected_name);
    let zion_name = format!("zion-{}-{}", expected_name, platform);
    let zion_exe_name = format!("zion-{}-{}.exe", expected_name, platform);

    for entry in archive.entries()? {
        let mut entry = entry.context("read tar entry")?;
        let path = entry.path().context("get tar entry path")?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Match the expected binary name (with or without .exe) or the
        // platform-specific release name used for miner assets (e.g.
        // zion-miner-linux-x86_64).
        if file_name == bin_name
            || file_name == expected_name
            || file_name == format!("{}.exe", expected_name)
            || file_name == zion_name
            || file_name == zion_exe_name
        {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).context("read tar entry content")?;
            fs::write(target, &bytes)
                .with_context(|| format!("write {} to {}", expected_name, target.display()))?;
            return Ok(());
        }
    }

    Err(anyhow!(
        "binary '{}' not found in tar.gz archive (expected file named '{}')",
        expected_name,
        bin_name
    ))
}

#[cfg(windows)]
fn extract_from_zip(data: &[u8], target: &Path, expected_name: &str, platform: &str) -> Result<()> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let reader = Cursor::new(data);
    let mut archive = ZipArchive::new(reader).context("open zip archive")?;

    let bin_name = binary_filename(expected_name);
    let zion_name = format!("zion-{}-{}", expected_name, platform);
    let zion_exe_name = format!("zion-{}-{}.exe", expected_name, platform);

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("read zip entry")?;
        let name = entry.name().to_string();
        let file_name = Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if file_name == bin_name
            || file_name == expected_name
            || file_name == zion_name
            || file_name == zion_exe_name
        {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes)
                .context("read zip entry content")?;
            fs::write(target, &bytes)
                .with_context(|| format!("write {} to {}", expected_name, target.display()))?;
            return Ok(());
        }
    }

    Err(anyhow!(
        "binary '{}' not found in zip archive (expected file named '{}')",
        expected_name,
        bin_name
    ))
}

#[cfg(not(windows))]
fn extract_from_zip(_data: &[u8], _target: &Path, _expected_name: &str, _platform: &str) -> Result<()> {
    Err(anyhow!("zip extraction is not available on this platform"))
}

/// Determine the current platform identifier for download URLs.
pub(crate) fn current_platform() -> Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("linux", "x86_64") => Ok("linux-x86_64".into()),
        ("linux", "aarch64") => Ok("linux-aarch64".into()),
        ("macos", "aarch64") => Ok("macos-aarch64".into()),
        ("macos", "x86_64") => Ok("macos-x86_64".into()),
        ("windows", "x86_64") => Ok("windows-x86_64".into()),
        _ => Err(anyhow!(
            "unsupported platform: {} {} — build from source: cargo build --release -p zion-public",
            os,
            arch
        )),
    }
}

fn binary_crate_name(name: &str) -> &'static str {
    match name {
        "node" => "core",
        "pool" => "pool",
        "miner" => "miner",
        _ => "core",
    }
}

// ─── Embedded binaries (Windows x86_64 only) ──────────────────────────────────

#[cfg(all(windows, target_arch = "x86_64"))]
fn bundled_bytes(name: &str) -> Result<&'static [u8]> {
    match name {
        "node" => Ok(include_bytes!("../../download/zion-node-windows-x86_64.exe")),
        "pool" => Ok(include_bytes!("../../download/zion-pool-windows-x86_64.exe")),
        "miner" => Ok(include_bytes!("../../download/zion-miner-windows-x86_64.exe")),
        _ => Err(anyhow!("no bundled binary for '{}'", name)),
    }
}
