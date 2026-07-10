//! Terminal UI helpers — colored output, headers, tables, prompts, genesis banner.

use colored::Colorize;
use std::io::{self, Write};

const GENESIS_TREE: &str = r#"
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡀⣀⢂⣁⣧⣖⡖⠠⢠⠀⠀⢤⡀⢀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢼⣶⡭⣛⠫⡞⠡⠀⡤⢦⠆⠨⠀⠀⢸⠋⠬⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡀⠀⠒⢈⠀⢭⣉⠂⡄⢠⠖⣸⠑⣆⡦⠊⢀⠀⡂⢉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⠍⠚⣁⣀⡀⣤⣰⢶⢷⢼⣿⠏⡡⢠⢗⡙⣶⣞⠛⣍⣪⣼⡠⠠⢶⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⠀⠀⠀⢄⣎⡠⢠⠉⠋⠓⠉⠋⢨⠘⠚⢉⡄⠁⢾⡌⣗⢿⠛⠲⠛⠋⡝⠑⠀⠌⡤⠄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠘⠥⠄⡚⣜⢣⣴⡨⢁⡀⣈⡅⠀⣀⠀⠈⣄⣀⢿⣯⡔⢊⢺⣷⠆⣷⠶⠂⠀⠀⠀⢀⡀⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠘⢁⣨⡅⠨⣤⣭⣵⣿⢿⢏⠿⠯⡁⠹⣿⡯⡜⠫⢯⢿⡾⣻⡅⣠⣆⣄⣰⡐⠲⠼⢶⠒⠯⠅⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠂⢈⠙⡋⣟⡛⣷⠴⢼⠓⠋⣺⣴⣷⣷⢾⣿⡿⣡⣠⣸⠗⠻⠹⠿⣟⢥⠯⣿⠻⢅⢴⢎⠄⠀⡄⢠⣀⠀⡀⠀⢄⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⢘⠳⠋⣤⣶⡿⢜⣳⢦⢶⣌⣩⠶⢠⣤⣯⠷⠈⠬⡉⠎⠎⣀⡌⠟⣝⣿⠇⡚⠒⠔⢀⣴⣍⣾⢲⠋⠟⠈⠙⠑⠉⢀⠄⠀
⠀⠀⠀⡀⣽⠿⠻⡈⠱⢻⣽⡟⣶⣚⡻⢏⢹⡋⠁⣀⣂⣤⣴⠄⢤⣐⣴⡾⣶⠯⣄⣉⢓⡭⢍⡆⡀⣈⣿⣷⡷⠶⠒⢂⣠⣠⢶⣾⣳⣯⣵⡄
⠀⠀⠀⠰⠴⠀⢘⢉⣧⣥⣏⠳⢈⣫⠞⣿⣷⢤⣤⣿⣿⣾⣧⣾⣿⣿⣿⣗⣿⣿⣿⠋⣚⡃⠿⡭⠹⣷⣿⠾⡿⢤⣤⣜⢿⣯⡿⣷⠯⣽⣿⡾
⠀⠀⠀⠀⠀⠐⠞⠻⣿⢟⣿⢿⠷⠥⣼⣷⢷⣯⠟⠻⠙⢉⡿⣿⢻⣹⣿⣿⢉⢳⣿⣿⣯⡶⡄⡶⢦⣷⣶⣿⡬⢥⠨⣭⣹⠏⠁⡘⢫⠉⠈⠀
⠀⠀⠔⣼⢂⠬⢌⠧⢋⡛⢡⣮⡡⠈⠓⣃⢀⣒⣊⣽⠻⣛⠟⢿⢸⣯⣿⣓⣿⡟⣷⣟⣿⣿⣿⣿⣻⣷⣟⣒⡺⠏⢰⡿⠿⣶⣶⡻⠒⡿⠦⡀
⠀⢆⣀⣆⣸⣿⠋⡴⢲⡁⡋⠀⢴⣮⣷⠟⠫⠿⣿⢶⢅⢴⣇⣸⣷⣿⣿⣧⣾⣿⣿⣿⣿⣿⣿⣿⣿⢿⢿⣟⣲⢦⠦⢋⡀⢿⣾⣷⣶⣤⠋⠆
⠈⠘⠛⠼⠿⡝⣻⠛⠻⠀⠀⠐⠛⢹⣱⣟⣽⣯⣿⡟⡊⣿⣷⣖⢽⣿⣿⣿⢿⣿⠀⠀⠘⠋⠃⠁⠀⠀⠨⠟⠿⡷⣥⣉⠁⠘⠉⠊⠚⠚⠓⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠋⠀⠀⠀⠀⠈⠋⠹⣎⢻⣿⠟⠀⠈⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠛⢳⡕⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⡄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣾⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣹⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠚⠛⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
"#;

const ZION_ASCII: &str = r#"
████████╗██╗ ██████╗███╗   ██╗     ██████╗██╗     ██╗
╚══███╔╝██║██╔═══██╗████╗  ██║    ██╔════╝██║     ██║
  ███╔╝ ██║██║   ██║██╔██╗ ██║    ██║     ██║     ██║
 ███╔╝  ██║██║   ██║██║╚██╗██║    ██║     ██║     ██║
███████╗██║╚██████╔╝██║ ╚████║    ╚██████╗███████╗██║
╚══════╝╚═╝ ╚═════╝ ╚═╝  ╚═══╝    ╚═════╝╚══════╝╚═╝
"#;

pub fn print_header(title: &str) {
    println!();
    println!("  {}", title.bold().bright_white());
    println!("  {}", "─".repeat(title.len()).dimmed());
}

pub fn print_ok(msg: &str) {
    println!("  {} {}", "✓".green().bold(), msg);
}

pub fn print_err(msg: &str) {
    eprintln!("  {} {}", "✗".red().bold(), msg);
}

pub fn print_warn(msg: &str) {
    println!("  {} {}", "⚠".yellow().bold(), msg);
}

pub fn print_info(msg: &str) {
    println!("  {} {}", "◉".cyan(), msg);
}

pub fn print_row(label: &str, value: &str) {
    println!("  {:<16} {}", label.dimmed(), value.bright_white());
}

pub fn wait_for_enter(msg: &str) -> io::Result<()> {
    print!("  {} {}", "↩".cyan(), msg.dimmed());
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    println!();
    Ok(())
}

pub fn print_section(title: &str) {
    println!();
    println!("  {}", title.dimmed().bold());
    println!("  {}", "─".repeat(40).dimmed());
}

/// Print the full genesis banner — tree + ZION CLI logo + tagline.
/// Shown on first launch of the interactive menu.
pub fn print_genesis_banner() {
    println!("{}", GENESIS_TREE.bright_yellow());
    println!();
    println!("{}", ZION_ASCII.bright_yellow());
    println!("  {}", "Public CLI · Community Edition".bright_white().bold());
    println!("  {}", "The Golden Age begins.".bright_cyan());
    println!(
        "  {} {} {}",
        "Om Namo Hiranyagarbha".dimmed(),
        "·".dimmed(),
        "Peace & One Love".dimmed()
    );
    println!();
}

/// Compact banner for subsequent menu returns (no tree).
pub fn print_compact_banner() {
    println!("{}", ZION_ASCII.bright_yellow());
    println!("  {}", "Public CLI · Community Edition".bright_white().bold());
    println!();
}

// ─── Dashboard ────────────────────────────────────────────────────────────────

use crate::commands::stats::Stats;

/// Print a live dashboard showing node, miner, pool, wallet status.
pub fn print_dashboard(s: &Stats) {
    println!("  {}", "┌─ Live Dashboard ──────────────────────────────────┐".cyan().dimmed());

    // Node
    let node_status = if let Some(pid) = s.node_process {
        format!("{} PID {}", "●".green(), pid)
    } else {
        format!("{} stopped", "○".red())
    };
    let node_height = s
        .node_height
        .map(|h| format!("#{}", h))
        .unwrap_or_else(|| if s.node_rpc_ok { "?".into() } else { "unreachable".into() });
    let node_peers = s
        .node_peers
        .map(|p| format!("{} peers", p))
        .unwrap_or_else(|| "-".into());
    println!(
        "  {} {:<8} {:<22} {:<18} {}",
        "│".cyan().dimmed(),
        "Node".bold(),
        node_status,
        node_height,
        node_peers
    );

    // Miner
    let miner_status = if let Some(pid) = s.miner_process {
        format!("{} PID {}", "●".green(), pid)
    } else {
        format!("{} stopped", "○".red())
    };
    let miner_hash = s
        .miner_stats
        .as_ref()
        .map(|m| format!("{:.1} H/s", m.hashrate_hps))
        .unwrap_or_else(|| "-".into());
    let miner_shares = s
        .miner_stats
        .as_ref()
        .map(|m| format!("✓{} ✗{}", m.accepted_shares, m.rejected_shares))
        .unwrap_or_else(|| "-".into());
    println!(
        "  {} {:<8} {:<22} {:<18} {}",
        "│".cyan().dimmed(),
        "Miner".bold(),
        miner_status,
        miner_hash,
        miner_shares
    );

    // Pool
    let pool_status = if let Some(pid) = s.pool_process {
        format!("{} PID {}", "●".green(), pid)
    } else {
        format!("{} public/none", "○".dimmed())
    };
    println!(
        "  {} {:<8} {}",
        "│".cyan().dimmed(),
        "Pool".bold(),
        pool_status
    );

    // Wallet
    let wallet_short: String = if s.wallet_address.is_empty() {
        "not set".red().to_string()
    } else if s.wallet_address.len() > 16 {
        format!("{}…{}", &s.wallet_address[..8], &s.wallet_address[s.wallet_address.len() - 6..])
    } else {
        s.wallet_address.clone()
    };
    let balance = s
        .wallet_balance
        .map(|b| format!("{:.6} ZION", b))
        .unwrap_or_else(|| "-".into());
    println!(
        "  {} {:<8} {:<22} {}",
        "│".cyan().dimmed(),
        "Wallet".bold(),
        wallet_short,
        balance
    );

    println!("  {}", "└───────────────────────────────────────────────────┘".cyan().dimmed());
    println!();
}

/// Print the help / start guide screen.
pub fn print_start_guide() {
    print_header("ZION Public CLI — Start Guide");

    println!("  {} What is ZION?", "1.".bold().bright_white());
    println!("     ZION is a community blockchain with its own node, pool,");
    println!("     and miner. You can run all of them from this one program.");
    println!();

    println!("  {} Quick start (3 steps):", "2.".bold().bright_white());
    println!("     {} Create a wallet:", "a)".cyan());
    println!("        Menu → Guided Setup → Step 1");
    println!("        (or: zion wallet new --mnemonic --set-default)");
    println!();
    println!("     {} Start mining:", "b)".cyan());
    println!("        Menu → Mine → Start autonomous mining");
    println!("        (or: zion mine start --auto-node)");
    println!();
    println!("     {} Check your progress:", "c)".cyan());
    println!("        Menu → Monitor");
    println!("        (or: zion monitor)");
    println!();

    println!("  {} The dashboard at the top shows:", "3.".bold().bright_white());
    println!("     {} Node  — is your local node running? current block height", "●".green());
    println!("     {} Miner — is your miner running? hashrate, accepted/rejected shares", "●".green());
    println!("     {} Pool  — are you using a local or public pool?", "●".green());
    println!("     {} Wallet — your address and balance", "●".green());
    println!();

    println!("  {} Useful commands:", "4.".bold().bright_white());
    println!("     zion wallet balance          Check your balance");
    println!("     zion wallet send --to ADDR   Send ZION to someone");
    println!("     zion node chain              See latest block info");
    println!("     zion node peers              See connected peers");
    println!("     zion mine stop               Stop mining");
    println!("     zion doctor                  Run diagnostics");
    println!("     zion config set KEY VALUE    Change a setting");
    println!();

    println!("  {} Tips:", "5.".bold().bright_white());
    println!("     {} Write down your 24-word mnemonic on paper. Never store it digitally.", "⚠".yellow());
    println!("     {} Mining on CPU is slow. GPU (opencl/cuda) is much faster.", "◉".cyan());
    println!("     {} The public pool at pool.zionterranova.com:8444 works without running a local node.", "◉".cyan());
    println!("     {} You can type any command directly from the menu: choose 'Run command'.", "◉".cyan());
    println!();
}
