pub mod commands;
pub mod config;
pub mod menu;
pub mod rpc;
pub mod ui;

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use commands::{mine, node, pool, wallet};

#[derive(Parser)]
#[command(
    name = "zion",
    about = "Zion CLI — wallet, node, and miner gateway for the ZION network",
    long_about = None,
    version,
    propagate_version = true,
)]
pub struct Cli {
    /// Config file (default: ~/.zion/zion.toml)
    #[arg(long, global = true)]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Open interactive arrow-key operator menu
    Menu,
    /// Print release metadata and manual update guidance
    Version,
    /// Check for and install the latest published CLI artifact
    Update {
        /// Only compare the local binary with the published artifact
        #[arg(long)]
        check: bool,
        /// Skip interactive confirmation and apply the update immediately
        #[arg(long)]
        yes: bool,
    },
    /// First-time setup wizard
    Onboard,
    /// Health check — node and pool
    Status,
    /// Run preflight diagnostics for config, local tools, and endpoints
    Doctor,

    /// L1 core node commands
    Node {
        #[command(subcommand)]
        cmd: node::NodeCmd,
    },
    /// L1 pool commands
    Pool {
        #[command(subcommand)]
        cmd: pool::PoolCmd,
    },
    /// L1 miner commands
    Mine {
        #[command(subcommand)]
        cmd: mine::MineCmd,
    },
    /// Wallet operations
    Wallet {
        #[command(subcommand)]
        cmd: wallet::WalletCmd,
    },
    /// Config management
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Print shell completion script
    Completions {
        /// Shell: bash | zsh | fish | powershell
        shell: Shell,
    },
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Print effective config
    Show,
    /// Set a config value
    Set { key: String, value: String },
    /// Show config file path
    Path,
    /// Validate current config values
    Validate,
    /// Re-run onboarding wizard
    Init,
}
