//! Interactive arrow-key menu — autonomous E2E workflow for public users.
//!
//! Features:
//! - Live dashboard at the top (node, miner, pool, wallet)
//! - Custom command input (type any `zion` subcommand)
//! - Help / Start Guide screen
//! - Guided setup: wallet → node → pool → miner → monitor

use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Input, Select};

use crate::commands::stats;
use crate::config::Config;
use crate::ui;

const BACK: &str = "← Back";
const EXIT: &str = "Exit";

/// Run the interactive menu. Returns `Some(args)` to dispatch, or `None` to exit.
pub async fn run(show_genesis: bool, cfg: &Config) -> Result<Option<Vec<String>>> {
    print_intro(show_genesis);

    loop {
        // Fetch live stats before each menu render.
        let s = stats::collect(cfg).await;
        ui::print_dashboard(&s);

        let items = [
            "🚀 Guided Setup — wallet → node → pool → miner",
            "Wallet — create, import, backup, balance, send",
            "Node — start, stop, status, query",
            "Pool — start, stop, status (solo/local mining)",
            "Mine — start, stop, status (auto-node mode)",
            "Monitor — detailed live view",
            "AI — chat with Hiran",
            "Status — network health check",
            "Doctor — preflight diagnostics",
            "Config — view / set values",
            "📖 Help / Start Guide",
            "⌨️  Run custom command (type it)",
            "Version",
            EXIT,
        ];

        let Some(choice) = select("ZION Public CLI", &items)? else {
            return Ok(None);
        };

        let selected = match choice {
            0 => Some(guided_setup_workflow()?),
            1 => wallet_menu()?,
            2 => node_menu()?,
            3 => pool_menu()?,
            4 => mine_menu()?,
            5 => Some(args(&["monitor"])),
            6 => ai_menu()?,
            7 => Some(args(&["status"])),
            8 => Some(args(&["doctor"])),
            9 => config_menu()?,
            10 => {
                ui::print_start_guide();
                wait_enter("Press Enter to return to the menu...")?;
                None
            }
            11 => Some(custom_command_input()?),
            12 => Some(args(&["version"])),
            13 => return Ok(None),
            _ => None,
        };

        if let Some(argv) = selected {
            return Ok(Some(argv));
        }
    }
}

fn print_intro(show_genesis: bool) {
    if show_genesis {
        ui::print_genesis_banner();
    } else {
        ui::print_compact_banner();
    }
    ui::print_info("Arrow keys navigate · Enter runs · Esc goes back");
    ui::print_info("Choose 'Help / Start Guide' if you're new · Choose 'Run command' to type any command");
    println!();
}

// ─── Custom Command Input ─────────────────────────────────────────────────────

fn custom_command_input() -> Result<Vec<String>> {
    ui::print_header("Run Custom Command");
    println!("  Type any zion command (without the 'zion' prefix).");
    println!("  Examples:");
    println!("    {}  wallet balance", "›".cyan());
    println!("    {}  node chain", "›".cyan());
    println!("    {}  mine start --auto-node --backend cpu", "›".cyan());
    println!("    {}  config set miner.wallet zion1...", "›".cyan());
    println!();

    let input = required_input("Command", None)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(args(&["menu"]));
    }

    // Split the input into args, handling quoted strings simply.
    let parts: Vec<String> = shell_split(input);
    let mut argv = vec!["zion".to_string()];
    argv.extend(parts);
    Ok(argv)
}

/// Simple shell-like splitting (handles double-quoted strings).
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

// ─── Guided Setup ─────────────────────────────────────────────────────────────

fn guided_setup_workflow() -> Result<Vec<String>> {
    ui::print_header("🚀 Guided Setup — Zero to Mining");

    let steps = [
        "Step 1: Create or import your wallet",
        "Step 2: Back up your mnemonic (CRITICAL)",
        "Step 3: Start local node",
        "Step 4: Start local pool (optional)",
        "Step 5: Start mining",
        "Step 6: Monitor everything",
        "Skip — back to main menu",
    ];

    loop {
        let Some(choice) = select("Guided Setup", &steps)? else {
            return Ok(args(&["menu"]));
        };

        let argv = match choice {
            0 => guided_step_wallet()?,
            1 => guided_step_backup()?,
            2 => guided_step_start_node()?,
            3 => guided_step_start_pool()?,
            4 => guided_step_mine()?,
            5 => args(&["monitor"]),
            _ => return Ok(args(&["menu"])),
        };

        if !argv.is_empty() {
            return Ok(argv);
        }
    }
}

fn guided_step_wallet() -> Result<Vec<String>> {
    ui::print_header("Step 1: Create or Import Your Wallet");
    println!("  You need a wallet to receive mining rewards and send ZION.");
    println!();

    let items = [
        "Create new wallet with mnemonic (recommended)",
        "Create new wallet (raw keypair, no mnemonic)",
        "Import existing mnemonic",
        "Import existing secret key (hex)",
        "I already have a wallet configured",
        BACK,
    ];

    let Some(choice) = select("Wallet Setup", &items)? else {
        return Ok(vec![]);
    };

    Ok(match choice {
        0 => wallet_new_args(true),
        1 => wallet_new_args(false),
        2 => wallet_import_mnemonic_args()?,
        3 => wallet_import_secret_key_args()?,
        4 => {
            ui::print_ok("Wallet already configured — proceeding to next step.");
            vec![]
        }
        _ => vec![],
    })
}

fn wallet_new_args(mnemonic: bool) -> Vec<String> {
    let out = optional_input("Output file path (blank = zion-wallet.json)", Some("zion-wallet.json")).unwrap_or_default();
    let mut argv = vec![
        "wallet".into(),
        "new".into(),
        "--out".into(),
        out,
        "--set-default".into(),
        "--print".into(),
    ];
    if mnemonic {
        argv.insert(2, "--mnemonic".into());
    }
    argv
}

fn wallet_import_mnemonic_args() -> Result<Vec<String>> {
    let mnemonic = required_input("Enter your mnemonic words", None)?;
    let out = optional_input("Output file path (blank = zion-wallet.json)", Some("zion-wallet.json"))?;
    Ok(args_owned(vec![
        "wallet".into(),
        "import-mnemonic".into(),
        "--mnemonic".into(),
        mnemonic,
        "--out".into(),
        out,
        "--set-default".into(),
        "--print".into(),
    ]))
}

fn wallet_import_secret_key_args() -> Result<Vec<String>> {
    let sk = required_input("Enter 32-byte secret key (64 hex chars)", None)?;
    let out = optional_input("Output file path (blank = zion-wallet.json)", Some("zion-wallet.json"))?;
    Ok(args_owned(vec![
        "wallet".into(),
        "import-secret-key".into(),
        "--secret-key-hex".into(),
        sk,
        "--out".into(),
        out,
        "--set-default".into(),
        "--print".into(),
    ]))
}

fn guided_step_backup() -> Result<Vec<String>> {
    ui::print_header("Step 2: Back Up Your Mnemonic");
    println!();
    println!("  {} If you used a mnemonic wallet, your 24 words are printed", "⚠".yellow().bold());
    println!("  above. Write them down on PAPER and store in a safe place.");
    println!();
    println!("  {} Anyone with these words can steal your funds.", "⚠".red().bold());
    println!("  {} Never share them. Never type them into a website.", "⚠".red().bold());
    println!("  {} Never store them digitally (photo, cloud, email).", "⚠".red().bold());
    println!();
    println!("  Commands:");
    println!("    zion wallet info --wallet zion-wallet.json");
    println!("    zion wallet reveal --wallet zion-wallet.json");
    println!();

    wait_enter("Press Enter when you've written down your mnemonic...")?;
    ui::print_ok("Backup confirmed. Proceeding to start node.");
    Ok(vec![])
}

fn guided_step_start_node() -> Result<Vec<String>> {
    ui::print_header("Step 3: Start Local Node");
    println!("  The node connects to the ZION network and lets your miner submit blocks.");
    println!("  You need the zion-node binary in the same folder or in ~/.zion/");
    println!();
    Ok(args(&["node", "start"]))
}

fn guided_step_start_pool() -> Result<Vec<String>> {
    ui::print_header("Step 4: Start Local Pool (Optional)");
    println!("  Most users should connect to the public pool (default).");
    println!("  Only run a local pool if you want solo/local mining.");
    println!();

    let items = ["Connect to public pool (skip)", "Start local pool", BACK];
    let Some(choice) = select("Pool", &items)? else {
        return Ok(vec![]);
    };

    Ok(match choice {
        0 => {
            ui::print_ok("Using public pool — proceed to Step 5.");
            vec![]
        }
        1 => args(&["pool", "start"]),
        _ => vec![],
    })
}

fn guided_step_mine() -> Result<Vec<String>> {
    ui::print_header("Step 5: Start Mining");
    println!("  Autonomous mode will start the local node first if needed, then the miner.");
    println!();

    let items = [
        "Autonomous: start node (if needed) + miner",
        "Start mining with GPU (auto-node)",
        "Start mining with custom settings (auto-node)",
        "Just show miner status",
        BACK,
    ];

    let Some(choice) = select("Start Mining", &items)? else {
        return Ok(vec![]);
    };

    Ok(match choice {
        0 => args(&["mine", "start", "--auto-node"]),
        1 => {
            let mut argv = guided_gpu_mine_start()?;
            argv.push("--auto-node".into());
            argv
        }
        2 => {
            let mut argv = guided_custom_mine_start()?;
            argv.push("--auto-node".into());
            argv
        }
        3 => args(&["mine", "status"]),
        _ => vec![],
    })
}

fn guided_gpu_mine_start() -> Result<Vec<String>> {
    let backends = ["opencl (AMD)", "cuda (NVIDIA)", "metal (Apple)"];
    let Some(idx) = select("GPU Backend", &backends)? else {
        return Ok(vec![]);
    };

    let backend = match idx {
        0 => "opencl",
        1 => "cuda",
        2 => "metal",
        _ => "opencl",
    };

    let worker = optional_input("Worker name (blank = worker-1)", Some("worker-1"))?;

    let mut argv = args(&["mine", "start"]);
    argv.push("--backend".into());
    argv.push(backend.into());
    if !worker.trim().is_empty() && worker != "worker-1" {
        argv.push("--worker".into());
        argv.push(worker);
    }
    Ok(argv)
}

fn guided_custom_mine_start() -> Result<Vec<String>> {
    let pool = optional_input("Pool host:port (blank = default)", None)?;
    let wallet = optional_input("Wallet override (blank = config)", None)?;
    let worker = optional_input("Worker name (blank = worker-1)", Some("worker-1"))?;

    let algos = [
        "deeksha_lite_v1 (default, balanced)",
        "cosmic_harmony_ekam_deeksha_v2 (heavy)",
        "deeksha_lite_fire (thermal, high power)",
    ];
    let Some(algo_idx) = select("Algorithm", &algos)? else {
        return Ok(vec![]);
    };
    let algorithm = match algo_idx {
        1 => "cosmic_harmony_ekam_deeksha_v2",
        2 => "deeksha_lite_fire",
        _ => "deeksha_lite_v1",
    };

    let backends = ["cpu", "opencl (AMD)", "cuda (NVIDIA)", "metal (Apple)"];
    let Some(be_idx) = select("Backend", &backends)? else {
        return Ok(vec![]);
    };
    let backend = match be_idx {
        1 => "opencl",
        2 => "cuda",
        3 => "metal",
        _ => "cpu",
    };

    let mut argv = args(&["mine", "start"]);
    argv.push("--algorithm".into());
    argv.push(algorithm.into());
    argv.push("--backend".into());
    argv.push(backend.into());
    if !pool.trim().is_empty() {
        argv.push("--pool".into());
        argv.push(pool);
    }
    if !wallet.trim().is_empty() {
        argv.push("--wallet".into());
        argv.push(wallet);
    }
    if !worker.trim().is_empty() && worker != "worker-1" {
        argv.push("--worker".into());
        argv.push(worker);
    }
    Ok(argv)
}

// ─── Wallet Menu ──────────────────────────────────────────────────────────────

fn wallet_menu() -> Result<Option<Vec<String>>> {
    loop {
        let items = [
            "Create new wallet (mnemonic)",
            "Create new wallet (raw keypair)",
            "Import mnemonic",
            "Import secret key (hex)",
            "Show wallet address",
            "Check balance",
            "Check balance (custom address)",
            "Send ZION",
            "Wallet file info",
            "Reveal wallet secrets",
            "Export wallet JSON",
            BACK,
        ];

        let Some(choice) = select("Wallet", &items)? else {
            return Ok(None);
        };

        let argv = match choice {
            0 => {
                let _ = optional_input("Output file (blank = zion-wallet.json)", Some("zion-wallet.json"))?;
                let set_default = confirm("Set as default miner wallet?")?;
                let mut argv = wallet_new_args(true);
                if !set_default { argv.retain(|x| x != "--set-default"); }
                Some(argv)
            }
            1 => {
                let _ = optional_input("Output file (blank = zion-wallet.json)", Some("zion-wallet.json"))?;
                let set_default = confirm("Set as default miner wallet?")?;
                let mut argv = wallet_new_args(false);
                if !set_default { argv.retain(|x| x != "--set-default"); }
                Some(argv)
            }
            2 => Some(wallet_import_mnemonic_args()?),
            3 => Some(wallet_import_secret_key_args()?),
            4 => Some(args(&["wallet", "address"])),
            5 => Some(args(&["wallet", "balance"])),
            6 => {
                let address = required_input("Address", None)?;
                Some(args_owned(vec!["wallet".into(), "balance".into(), "--address".into(), address]))
            }
            7 => guided_wallet_send()?,
            8 => {
                let wallet = optional_input("Wallet file (blank = zion-wallet.json)", Some("zion-wallet.json"))?;
                Some(args_owned(vec!["wallet".into(), "info".into(), "--wallet".into(), wallet]))
            }
            9 => {
                let wallet = optional_input("Wallet file (blank = zion-wallet.json)", Some("zion-wallet.json"))?;
                let pw_env = optional_input("Password env var (blank = none)", None)?;
                let mut argv = args_owned(vec!["wallet".into(), "reveal".into(), "--wallet".into(), wallet]);
                if !pw_env.trim().is_empty() {
                    argv.push("--password-env".into());
                    argv.push(pw_env);
                }
                Some(argv)
            }
            10 => {
                let wallet = optional_input("Wallet file (blank = zion-wallet.json)", Some("zion-wallet.json"))?;
                Some(args_owned(vec!["wallet".into(), "export".into(), "--wallet".into(), wallet]))
            }
            _ => return Ok(None),
        };

        if let Some(a) = argv {
            return Ok(Some(a));
        }
    }
}

fn guided_wallet_send() -> Result<Option<Vec<String>>> {
    let to = required_input("Recipient address (zion1...)", None)?;
    let amount = required_input("Amount in ZION", None)?;
    let memo = optional_input("Memo (optional)", None)?;
    let wallet = optional_input("Wallet file (blank = zion-wallet.json)", Some("zion-wallet.json"))?;

    let mut argv = args_owned(vec![
        "wallet".into(),
        "send".into(),
        "--to".into(),
        to,
        "--amount".into(),
        amount,
        "--wallet".into(),
        wallet,
    ]);
    if !memo.trim().is_empty() {
        argv.push("--memo".into());
        argv.push(memo);
    }
    Ok(Some(argv))
}

// ─── Node Menu ──────────────────────────────────────────────────────────────────

fn node_menu() -> Result<Option<Vec<String>>> {
    let items = [
        "Start local node",
        "Stop local node",
        "Local node process status",
        "Node info (RPC query)",
        "Chain info (height, tip, mempool)",
        "Connected peers",
        "Supply info (total, mined, remaining)",
        "Mempool details",
        BACK,
    ];

    let Some(choice) = select("Node", &items)? else {
        return Ok(None);
    };

    Ok(match choice {
        0 => Some(args(&["node", "start"])),
        1 => Some(args(&["node", "stop"])),
        2 => Some(args(&["node", "status"])),
        3 => Some(args(&["node", "info"])),
        4 => Some(args(&["node", "chain"])),
        5 => Some(args(&["node", "peers"])),
        6 => Some(args(&["node", "supply"])),
        7 => Some(args(&["node", "mempool"])),
        _ => None,
    })
}

// ─── Pool Menu ──────────────────────────────────────────────────────────────────

fn pool_menu() -> Result<Option<Vec<String>>> {
    let items = [
        "Start local pool",
        "Stop local pool",
        "Local pool process status",
        BACK,
    ];

    let Some(choice) = select("Pool", &items)? else {
        return Ok(None);
    };

    Ok(match choice {
        0 => Some(args(&["pool", "start"])),
        1 => Some(args(&["pool", "stop"])),
        2 => Some(args(&["pool", "status"])),
        _ => None,
    })
}

// ─── Mine Menu ──────────────────────────────────────────────────────────────────

fn mine_menu() -> Result<Option<Vec<String>>> {
    loop {
        let items = [
            "Start autonomous mining (auto-starts node)",
            "Start mining (GPU guided, auto-node)",
            "Start mining (custom guided, auto-node)",
            "Start mining (public pool only)",
            "Miner status (hashrate, shares, uptime)",
            "Live monitor dashboard (refreshes every 2s)",
            "View miner log (last 50 lines)",
            "Follow miner log (real-time, Ctrl+C to stop)",
            "Stop miner",
            BACK,
        ];

        let Some(choice) = select("Mining", &items)? else {
            return Ok(None);
        };

        let argv = match choice {
            0 => Some(args(&["mine", "start", "--auto-node"])),
            1 => {
                let mut argv = guided_gpu_mine_start()?;
                argv.push("--auto-node".into());
                Some(argv)
            }
            2 => {
                let mut argv = guided_custom_mine_start()?;
                argv.push("--auto-node".into());
                Some(argv)
            }
            3 => Some(args(&["mine", "start"])),
            4 => Some(args(&["mine", "status"])),
            5 => Some(args(&["mine", "monitor"])),
            6 => Some(args(&["mine", "log", "--lines", "50"])),
            7 => Some(args(&["mine", "log", "--follow"])),
            8 => Some(args(&["mine", "stop"])),
            _ => return Ok(None),
        };

        if let Some(a) = argv {
            return Ok(Some(a));
        }
    }
}

// ─── AI Menu ──────────────────────────────────────────────────────────────────

fn ai_menu() -> Result<Option<Vec<String>>> {
    let items = [
        "Chat with Hiran (interactive)",
        "Ask one question",
        "Hiran AI status",
        BACK,
    ];

    let Some(choice) = select("Hiran AI", &items)? else {
        return Ok(None);
    };

    Ok(match choice {
        0 => Some(args(&["ai", "chat"])),
        1 => {
            let question = required_input("Your question", None)?;
            Some(args_owned(vec!["ai".into(), "ask".into(), question]))
        }
        2 => Some(args(&["ai", "status"])),
        _ => None,
    })
}

// ─── Config Menu ──────────────────────────────────────────────────────────────

fn config_menu() -> Result<Option<Vec<String>>> {
    loop {
        let items = [
            "Set miner wallet address",
            "Set node RPC endpoint",
            "Set node P2P bind",
            "Set node seed peers",
            "Set pool endpoint",
            "Set pool bind (local)",
            "Set AI endpoint",
            "Set miner algorithm",
            "Set miner backend",
            "Set worker name",
            "Toggle auto-start node for miner",
            "Toggle auto-start pool for miner",
            "Set binary paths",
            "Show config file path",
            BACK,
        ];

        let Some(choice) = select("Config", &items)? else {
            return Ok(None);
        };

        let argv = match choice {
            0 => {
                let val = required_input("Wallet address (zion1...)", None)?;
                Some(args_owned(vec!["config".into(), "set".into(), "miner.wallet".into(), val]))
            }
            1 => {
                let host = required_input("RPC host", Some("rpc.zionterranova.com"))?;
                Some(args_owned(vec!["config".into(), "set".into(), "node.rpc_host".into(), host]))
            }
            2 => {
                let val = required_input("P2P bind", Some("0.0.0.0:8333"))?;
                Some(args_owned(vec!["config".into(), "set".into(), "node.p2p_bind".into(), val]))
            }
            3 => {
                let val = required_input("Seed peers", Some("rpc.zionterranova.com:8333"))?;
                Some(args_owned(vec!["config".into(), "set".into(), "node.seed_peers".into(), val]))
            }
            4 => {
                let host = required_input("Pool host", Some("pool.zionterranova.com"))?;
                Some(args_owned(vec!["config".into(), "set".into(), "pool.host".into(), host]))
            }
            5 => {
                let val = required_input("Pool bind", Some("0.0.0.0:8444"))?;
                Some(args_owned(vec!["config".into(), "set".into(), "pool.bind".into(), val]))
            }
            6 => {
                let url = required_input("AI endpoint URL (blank = disabled)", Some(""))?;
                Some(args_owned(vec!["config".into(), "set".into(), "ai.url".into(), url]))
            }
            7 => {
                let algos = ["deeksha_lite_v1", "cosmic_harmony_ekam_deeksha_v2", "deeksha_lite_fire"];
                let Some(idx) = select("Algorithm", &algos)? else { return Ok(None); };
                Some(args_owned(vec!["config".into(), "set".into(), "miner.algorithm".into(), algos[idx].into()]))
            }
            8 => {
                let backends = ["cpu", "opencl", "cuda", "metal"];
                let Some(idx) = select("Backend", &backends)? else { return Ok(None); };
                Some(args_owned(vec!["config".into(), "set".into(), "miner.backend".into(), backends[idx].into()]))
            }
            9 => {
                let name = required_input("Worker name", Some("worker-1"))?;
                Some(args_owned(vec!["config".into(), "set".into(), "miner.worker_name".into(), name]))
            }
            10 => {
                let val = if confirm("Auto-start node when miner starts?")? { "true" } else { "false" };
                Some(args_owned(vec!["config".into(), "set".into(), "miner.auto_start_node".into(), val.into()]))
            }
            11 => {
                let val = if confirm("Auto-start pool when miner starts?")? { "true" } else { "false" };
                Some(args_owned(vec!["config".into(), "set".into(), "miner.auto_start_pool".into(), val.into()]))
            }
            12 => binary_paths_menu()?,
            13 => Some(args(&["config", "path"])),
            _ => return Ok(None),
        };

        if let Some(a) = argv {
            return Ok(Some(a));
        }
    }
}

fn binary_paths_menu() -> Result<Option<Vec<String>>> {
    let items = [
        "Set node binary path",
        "Set pool binary path",
        "Set miner binary path",
        BACK,
    ];
    let Some(choice) = select("Binary Paths", &items)? else {
        return Ok(None);
    };

    Ok(match choice {
        0 => {
            let path = required_input("Path to node binary", Some("zion-node-windows-x86_64.exe"))?;
            Some(args_owned(vec!["config".into(), "set".into(), "binaries.node".into(), path]))
        }
        1 => {
            let path = required_input("Path to pool binary", Some("zion-pool-windows-x86_64.exe"))?;
            Some(args_owned(vec!["config".into(), "set".into(), "binaries.pool".into(), path]))
        }
        2 => {
            let path = required_input("Path to miner binary", Some("zion-miner-windows-x86_64.exe"))?;
            Some(args_owned(vec!["config".into(), "set".into(), "binaries.miner".into(), path]))
        }
        _ => None,
    })
}

// ─── Helpers ────────────────────────────────────────────────────────────────────

fn select(prompt: &str, items: &[&str]) -> Result<Option<usize>> {
    Ok(Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact_opt()?)
}

fn required_input(prompt: &str, initial: Option<&str>) -> Result<String> {
    let theme = ColorfulTheme::default();
    let mut input = Input::<String>::with_theme(&theme).with_prompt(prompt);
    if let Some(initial) = initial {
        input = input.default(initial.to_string());
    }
    Ok(input.interact_text()?)
}

fn optional_input(prompt: &str, initial: Option<&str>) -> Result<String> {
    let theme = ColorfulTheme::default();
    let mut input = Input::<String>::with_theme(&theme)
        .with_prompt(prompt)
        .allow_empty(true);
    if let Some(initial) = initial {
        input = input.default(initial.to_string());
    }
    Ok(input.interact_text()?)
}

fn confirm(prompt: &str) -> Result<bool> {
    let theme = ColorfulTheme::default();
    let items = ["Yes", "No"];
    let choice = Select::with_theme(&theme)
        .with_prompt(prompt)
        .items(&items)
        .default(0)
        .interact_opt()?;
    Ok(choice == Some(0))
}

fn wait_enter(prompt: &str) -> Result<()> {
    use std::io::{self, Write};
    print!("\n  {} ", prompt.dimmed());
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(())
}

fn args(parts: &[&str]) -> Vec<String> {
    let mut argv = vec!["zion".to_string()];
    argv.extend(parts.iter().map(|p| (*p).to_string()));
    argv
}

fn args_owned(parts: Vec<String>) -> Vec<String> {
    let mut argv = vec!["zion".to_string()];
    argv.extend(parts);
    argv
}
