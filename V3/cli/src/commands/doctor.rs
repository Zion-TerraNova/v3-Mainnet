use anyhow::{Context, Result};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
use std::time::Duration;

use crate::commands::mine;
use crate::config::{self, Config};
use crate::rpc::node_rpc;
use crate::ui;

pub async fn run(cfg: &Config) -> Result<()> {
    ui::print_banner();
    ui::print_header("Doctor");

    let mut hard_failures = 0usize;

    ui::print_header("Config");
    let report = config::validate(cfg);
    for warning in &report.warnings {
        ui::print_warn(warning);
    }
    for error in &report.errors {
        ui::print_err(error);
    }
    if report.is_ok() {
        ui::print_ok("Config schema and value checks passed");
    } else {
        hard_failures += report.errors.len();
    }

    ui::print_header("Local Runtime");
    let config_path = config::config_path()?;
    if config_path.exists() {
        ui::print_ok(&format!("Config file    {}", config_path.display()));
    } else {
        ui::print_warn(&format!(
            "Config file    missing at {}; using built-in defaults",
            config_path.display()
        ));
    }

    match mine::discover_miner_binary() {
        Some(path) => ui::print_ok(&format!("Miner binary  {}", path.display())),
        None => ui::print_warn("Miner binary  zion-miner not found locally; build with `cd V3 && cargo build -p zion-miner --release`")
    }

    ui::print_header("Mining Environment");
    if cfg.miner.wallet.trim().is_empty() {
        ui::print_warn(
            "Mining wallet  not configured; set with `zion config set miner.wallet <address>`",
        );
    } else {
        ui::print_ok("Mining wallet  configured");
    }

    match validate_threads_setting(&cfg.miner.threads) {
        Ok(message) => ui::print_ok(&format!("Miner threads  {}", message)),
        Err(message) => {
            hard_failures += 1;
            ui::print_err(&format!("Miner threads  {}", message));
        }
    }

    match backend_runtime_note(&cfg.miner.backend) {
        BackendDoctorNote::Ok(message) => ui::print_ok(&format!("Miner backend  {}", message)),
        BackendDoctorNote::Warn(message) => ui::print_warn(&format!("Miner backend  {}", message)),
    }

    match validate_algorithm_setting(&cfg.miner.algorithm) {
        Ok(message) => ui::print_ok(&format!("Miner algorithm {}", message)),
        Err(message) => ui::print_warn(&format!("Miner algorithm {}", message)),
    }

    if cfg.miner.profile.trim().eq_ignore_ascii_case("dual") {
        if cfg.miner.btc_wallet.trim().is_empty() {
            ui::print_warn("Dual profile   miner.btc_wallet is empty; DCR sidecar payout is not fully configured");
        } else {
            ui::print_ok("Dual profile   BTC payout wallet configured");
        }
    }

    match tcp_probe(&cfg.pool.host, cfg.pool.port, Duration::from_secs(3)) {
        Ok(()) => ui::print_ok(&format!(
            "Pool target    {}:{} reachable",
            cfg.pool.host, cfg.pool.port
        )),
        Err(err) => ui::print_warn(&format!(
            "Pool target    {}:{} — {}",
            cfg.pool.host, cfg.pool.port, err
        )),
    }

    ui::print_header("Node Endpoints");
    let (host, port) = cfg.rpc();
    match node_rpc::call0(host, port, "getChainInfo").await {
        Ok(v) => {
            let height = v["chain_height"].as_u64().unwrap_or(0);
            let hash = v["tip_hash"].as_str().unwrap_or("?");
            let short = if hash.len() > 12 { &hash[..12] } else { hash };
            ui::print_ok(&format!(
                "Node RPC      {}:{} height={} tip={}...",
                host, port, height, short
            ));
        }
        Err(err) => {
            ui::print_warn(&format!(
                "Node RPC      {}:{} — {}",
                host, port, err
            ));
        }
    }

    println!();
    if hard_failures == 0 {
        ui::print_ok("Doctor passed");
        Ok(())
    } else {
        anyhow::bail!("Doctor found {} hard failure(s)", hard_failures)
    }
}

enum BackendDoctorNote {
    Ok(String),
    Warn(String),
}

fn validate_algorithm_setting(algorithm: &str) -> Result<String, String> {
    let trimmed = algorithm.trim();
    if trimmed.is_empty() {
        return Ok("deeksha_lite_v1 (default)".to_string());
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "deeksha_lite_v1" | "lite" | "dl" | "dlv1" => Ok("deeksha_lite_v1".to_string()),
        "deeksha_lite_fire" | "fire" | "dlfire" => Ok("deeksha_lite_fire (thermal-intensive)".to_string()),
        "cosmic_harmony_ekam_deeksha_v2" | "ekam" | "ekam_v2" | "full" => Ok("cosmic_harmony_ekam_deeksha_v2".to_string()),
        other => Err(format!("has unsupported value '{}'; use: deeksha_lite_v1, deeksha_lite_fire, cosmic_harmony_ekam_deeksha_v2", other)),
    }
}

fn validate_threads_setting(threads: &str) -> Result<String, String> {
    let trimmed = threads.trim();
    if trimmed.eq_ignore_ascii_case("auto") {
        return Ok("auto".to_string());
    }

    match trimmed.parse::<usize>() {
        Ok(0) => Err("must be greater than 0 or `auto`".to_string()),
        Ok(value) => Ok(value.to_string()),
        Err(_) => Err(format!(
            "has unsupported value '{}'; use a positive integer or `auto`",
            trimmed
        )),
    }
}

fn backend_runtime_note(backend: &str) -> BackendDoctorNote {
    match backend.trim().to_ascii_lowercase().as_str() {
        "auto" | "cpu" => BackendDoctorNote::Ok(backend.trim().to_string()),
        "gpu" => BackendDoctorNote::Warn(
            "generic gpu selected; runtime support depends on the actual miner host".to_string(),
        ),
        "metal" => {
            if cfg!(target_os = "macos") {
                BackendDoctorNote::Ok("metal requested on macOS host".to_string())
            } else {
                BackendDoctorNote::Warn(
                    "metal is configured but this host is not macOS".to_string(),
                )
            }
        }
        "opencl" | "ocl" => {
            if command_exists("clinfo") || cfg!(target_os = "macos") {
                BackendDoctorNote::Ok("opencl runtime probe passed".to_string())
            } else {
                BackendDoctorNote::Warn("opencl selected but `clinfo` was not found; verify OpenCL runtime on the miner host".to_string())
            }
        }
        "cuda" => {
            if command_exists("nvidia-smi") {
                BackendDoctorNote::Ok("cuda runtime probe passed via nvidia-smi".to_string())
            } else {
                BackendDoctorNote::Warn("cuda selected but `nvidia-smi` was not found; verify NVIDIA runtime on the miner host".to_string())
            }
        }
        other => {
            BackendDoctorNote::Warn(format!("unsupported backend '{}' in runtime probe", other))
        }
    }
}

fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn tcp_probe(host: &str, port: u16, timeout: Duration) -> Result<()> {
    let address = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("could not resolve {}:{}", host, port))?
        .next()
        .with_context(|| format!("no socket address resolved for {}:{}", host, port))?;

    TcpStream::connect_timeout(&address, timeout)
        .with_context(|| format!("connect failed to {}:{}", host, port))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_threads_setting};

    #[test]
    fn thread_setting_accepts_auto_and_positive_values() {
        assert_eq!(
            validate_threads_setting("auto").expect("auto should pass"),
            "auto"
        );
        assert_eq!(validate_threads_setting("8").expect("8 should pass"), "8");
    }

    #[test]
    fn thread_setting_rejects_zero_and_garbage() {
        assert!(validate_threads_setting("0").is_err());
        assert!(validate_threads_setting("many").is_err());
    }
}
