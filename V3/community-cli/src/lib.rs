//! **ZION Public CLI** — community gateway to the ZION network.
//!
//! Slim, safe subset of the operator CLI. Contains only what a public user needs:
//! - `wallet` — create, import, balance, send, address
//! - `node` — read-only chain / peers / supply / info queries
//! - `mine` — start / stop / status (local miner subprocess)
//! - `ai` — chat with Hiran + status check
//! - `status` — health check of public endpoints (node, pool, explorer, AI)
//! - `doctor` — preflight diagnostics for local environment
//!
//! **No deploy, no DAO, no bridge, no swap, no topology, no internal services.**

pub mod bundle;
pub mod commands;
pub mod config;
pub mod menu;
pub mod process;
pub mod ui;

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use commands::{ai, mine, node, pool, wallet};

/// ZION Public CLI — community edition.
#[derive(Parser)]
#[command(
    name = "zion",
    about = "ZION Public CLI — wallet, node, miner, AI & diagnostics for the community",
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
    /// Open interactive arrow-key menu
    Menu,

    /// Print version and build metadata
    Version,

    /// Wallet operations: create, import, balance, send
    Wallet {
        #[command(subcommand)]
        cmd: wallet::WalletCmd,
    },

    /// Read-only node queries: info, peers, chain, supply
    Node {
        #[command(subcommand)]
        cmd: node::NodeCmd,
    },

    /// Local miner control: start, stop, status
    Mine {
        #[command(subcommand)]
        cmd: mine::MineCmd,
    },

    /// Local mining pool control: start, stop, status
    Pool {
        #[command(subcommand)]
        cmd: pool::PoolCmd,
    },

    /// Hiran AI — chat and status
    Ai {
        #[command(subcommand)]
        cmd: ai::AiCmd,
    },

    /// Health check — public endpoints (node, pool, explorer, AI)
    Status,

    /// Preflight diagnostics for local environment
    Doctor,

    /// Live monitor — node, pool, miner + chain height
    Monitor,

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
    /// Set a config value (key value)
    Set { key: String, value: String },
    /// Show config file path
    Path,
}
