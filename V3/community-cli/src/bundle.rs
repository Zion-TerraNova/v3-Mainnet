//! Self-contained binary bundle — embeds the ZION node, pool, and miner
//! executables directly into the `zion` CLI binary so users only need one file.
//!
//! On first use of a bundled service, the executable bytes are extracted to
//! `~/.zion/bin/<name>.exe` (Windows) or `~/.zion/bin/<name>` (Unix). The
//! extraction only happens if the file is missing, so repeated runs are fast.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Return the path to a usable bundled binary, extracting it first if needed.
///
/// Supported names: `node`, `pool`, `miner`.
/// On Windows the extracted file has `.exe` extension.
pub fn ensure_binary(name: &str) -> Result<PathBuf> {
    let dir = bundle_dir()?;
    let target = dir.join(binary_filename(name));

    if !target.exists() {
        extract_binary(name, &target)?;
    }

    Ok(target)
}

/// Force re-extraction of a bundled binary.
/// Useful for `zion doctor --extract` or after a CLI update.
pub fn extract_all() -> Result<Vec<(String, PathBuf)>> {
    let names = ["node", "pool", "miner"];
    let mut out = Vec::new();
    for name in names {
        let path = ensure_binary(name)?;
        out.push((name.to_string(), path));
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

fn extract_binary(name: &str, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    let bytes = bundled_bytes(name)?;
    fs::write(target, bytes).with_context(|| format!("extract {} to {}", name, target.display()))?;

    Ok(())
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn bundled_bytes(name: &str) -> Result<&'static [u8]> {
    match name {
        "node" => Ok(include_bytes!("../../download/zion-node-windows-x86_64.exe")),
        "pool" => Ok(include_bytes!("../../download/zion-pool-windows-x86_64.exe")),
        "miner" => Ok(include_bytes!("../../download/zion-miner-windows-x86_64.exe")),
        _ => Err(anyhow!("no bundled binary for '{}'", name)),
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn bundled_bytes(name: &str) -> Result<&'static [u8]> {
    Err(anyhow!(
        "no bundled binary for '{}' on this platform (only Windows x86_64 bundle is shipped)",
        name
    ))
}
