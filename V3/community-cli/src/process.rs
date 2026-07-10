//! Generic process manager for local ZION services (node, pool, miner).
//!
//! Each managed service writes its PID to `~/.zion/<name>.pid`. The CLI can
//! later read the PID, check if the process is alive, and stop it.

use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Start a managed service and record its PID.
///
/// * `name` — service name, used for PID file (`~/.zion/<name>.pid`)
/// * `bin` — absolute path to the binary
/// * `args` — command-line arguments
/// * `envs` — extra environment variables
/// * `console` — if true, spawn a visible console window on Windows
pub fn start(
    name: &str,
    bin: &Path,
    args: &[String],
    envs: &[(&str, String)],
    console: bool,
) -> Result<u32> {
    if !bin.exists() {
        return Err(anyhow!("binary not found: {}", bin.display()));
    }

    // Stop any stale PID first.
    if let Some(pid) = read_pid(name) {
        if is_alive(pid) {
            return Err(anyhow!(
                "{} is already running (PID {}). Run stop first.",
                name,
                pid
            ));
        } else {
            clear_pid(name);
        }
    }

    let mut cmd = Command::new(bin);
    cmd.args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }

    if console {
        show_console(&mut cmd);
    } else {
        // Run in background. On Windows we use DETACHED_PROCESS so the child
        // survives if the CLI console exits; on Unix we just discard stdio.
        detach(&mut cmd);
    }

    let child = cmd.spawn().with_context(|| format!("failed to start {}", name))?;
    let pid = child.id();
    write_pid(name, pid)?;

    // Detach on non-Windows too by forgetting the handle.
    std::mem::forget(child);

    Ok(pid)
}

/// Stop a managed service by PID.
pub fn stop(name: &str) -> Result<bool> {
    let pid = match read_pid(name) {
        Some(p) => p,
        None => return Ok(false),
    };

    if !is_alive(pid) {
        clear_pid(name);
        return Ok(false);
    }

    kill(pid)?;
    clear_pid(name);
    Ok(true)
}

/// Check if a managed service is currently running.
pub fn status(name: &str) -> Option<u32> {
    let pid = read_pid(name)?;
    if is_alive(pid) {
        Some(pid)
    } else {
        clear_pid(name);
        None
    }
}

/// Read PID file for a service.
fn read_pid(name: &str) -> Option<u32> {
    let path = pid_path(name).ok()?;
    let raw = fs::read_to_string(&path).ok()?;
    raw.trim().parse::<u32>().ok()
}

/// Write PID file for a service.
fn write_pid(name: &str, pid: u32) -> Result<()> {
    let path = pid_path(name)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, pid.to_string())?;
    Ok(())
}

/// Clear PID file for a service.
pub fn clear_pid(name: &str) {
    if let Ok(path) = pid_path(name) {
        let _ = fs::remove_file(&path);
    }
}

/// PID file path: `~/.zion/<name>.pid`
fn pid_path(name: &str) -> Result<PathBuf> {
    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| anyhow!("cannot determine home directory"))?;
    Ok(PathBuf::from(home).join(".zion").join(format!("{}.pid", name)))
}

/// Check if a process is alive.
fn is_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains(&pid.to_string())
            }
            Err(_) => false,
        }
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Kill a process by PID.
fn kill(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|e| anyhow!("failed to kill process {}: {}", pid, e))?;
    }
    #[cfg(unix)]
    {
        Command::new("kill")
            .arg(pid.to_string())
            .output()
            .map_err(|e| anyhow!("failed to kill process {}: {}", pid, e))?;
    }
    Ok(())
}

/// Configure a command to spawn in a new detached console on Windows,
/// or detached in the background on Unix.
fn detach(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        // DETACHED_PROCESS keeps the process alive if the parent console exits.
        // CREATE_NEW_PROCESS_GROUP lets us stop the whole tree with taskkill /T.
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::process::Stdio;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
}

/// Configure a command to spawn in a new visible console window.
/// On Windows this is `CREATE_NEW_CONSOLE`; on Unix it's a no-op.
fn show_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP);
    }
}

/// Find a binary in common locations.
///
/// Searches: current dir, ~/.zion/, PATH, and <repo>/target/release.
/// On Windows, appends `.exe` if needed.
pub fn find_binary(name: &str) -> Option<PathBuf> {
    let bin_name = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    // 1. Current directory
    let candidate = PathBuf::from(".").join(&bin_name);
    if candidate.exists() {
        return Some(candidate);
    }

    // 2. Directory of the current executable (highest priority for downloaded bundles)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(&bin_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 3. ~/.zion/bin/ (extracted bundled binaries)
    if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
        let candidate = PathBuf::from(home).join(".zion").join("bin").join(&bin_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 4. ~/.zion/
    if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
        let candidate = PathBuf::from(home).join(".zion").join(&bin_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 5. PATH (lowest priority — avoid matching generic names like `node` from Node.js)
    if let Ok(path_var) = env::var("PATH") {
        for dir in path_var.split(if cfg!(windows) { ';' } else { ':' }) {
            let candidate = PathBuf::from(dir).join(&bin_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 5. repo/target/release (common sibling of the CLI binary)
    let candidate = PathBuf::from("..").join("..").join("target").join("release").join(&bin_name);
    if candidate.exists() {
        return Some(candidate);
    }

    None
}

/// Like `find_binary`, but never searches PATH — only current dir, exe dir, and ~/.zion.
/// Use for binaries with generic names (e.g., `node`, `server`) to avoid picking up
/// unrelated system tools.
pub fn find_binary_safely(name: &str) -> Option<PathBuf> {
    let bin_name = if cfg!(windows) && !name.ends_with(".exe") {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };

    // 1. Current directory
    let candidate = PathBuf::from(".").join(&bin_name);
    if candidate.exists() {
        return Some(candidate);
    }

    // 2. Directory of the current executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidate = exe_dir.join(&bin_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 3. ~/.zion/bin/
    if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
        let candidate = PathBuf::from(home).join(".zion").join("bin").join(&bin_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 4. ~/.zion/
    if let Ok(home) = env::var("HOME").or_else(|_| env::var("USERPROFILE")) {
        let candidate = PathBuf::from(home).join(".zion").join(&bin_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

/// Check if a binary exists and return its canonical path.
pub fn resolve_binary(name: &str) -> Result<PathBuf> {
    find_binary(name).ok_or_else(|| {
        anyhow!(
            "{} not found. Download it from https://zionterranova.com/download or build with: cargo build --release -p {}",
            name,
            name
        )
    })
}
