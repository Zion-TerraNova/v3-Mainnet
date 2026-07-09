use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, Input, Select};

use crate::ui;

const BACK: &str = "<- Back";
const EXIT: &str = "Exit";

pub fn run(show_genesis: bool) -> Result<Option<Vec<String>>> {
    print_intro(show_genesis);

    loop {
        let items = [
            "Health & fast checks",
            "L1 node & pool",
            "Mining & wallet",
            "Config",
            "Onboarding",
            EXIT,
        ];

        let Some(choice) = select("ZION operator menu", &items)? else {
            return Ok(None);
        };

        let selected = match choice {
            0 => quick_status_menu()?,
            1 => l1_menu()?,
            2 => mining_wallet_menu()?,
            3 => config_menu()?,
            4 => Some(args(&["onboard"])),
            5 => return Ok(None),
            _ => None,
        };

        if selected.is_some() {
            return Ok(selected);
        }
    }
}

fn print_intro(show_genesis: bool) {
    if show_genesis {
        ui::print_genesis_banner();
    } else {
        ui::print_header("ZION operator menu");
    }
    ui::print_info("Arrow keys navigate, Enter runs, Esc leaves the current menu.");
    ui::print_row("Health", "doctor, status, node, pool");
    ui::print_row("L1", "node and pool inspection");
    ui::print_row("Mine", "miner start, bench, stop, wallet send");
    println!();
}

fn l1_menu() -> Result<Option<Vec<String>>> {
    loop {
        let items = ["Node", "Pool", BACK];

        let Some(choice) = select("L1 node & pool", &items)? else {
            return Ok(None);
        };

        let selected = match choice {
            0 => node_menu()?,
            1 => pool_menu()?,
            _ => return Ok(None),
        };

        if selected.is_some() {
            return Ok(selected);
        }
    }
}

fn mining_wallet_menu() -> Result<Option<Vec<String>>> {
    loop {
        let items = ["Mining", "Wallet", BACK];

        let Some(choice) = select("Mining & wallet", &items)? else {
            return Ok(None);
        };

        let selected = match choice {
            0 => mine_menu()?,
            1 => wallet_menu()?,
            _ => return Ok(None),
        };

        if selected.is_some() {
            return Ok(selected);
        }
    }
}

fn quick_status_menu() -> Result<Option<Vec<String>>> {
    {
        let items = [
            "zion status",
            "zion doctor",
            "zion node status",
            "zion pool stats",
            "zion wallet balance",
            "zion mine status",
            BACK,
        ];

        let Some(choice) = select("Quick status", &items)? else {
            return Ok(None);
        };

        Ok(match choice {
            0 => Some(args(&["status"])),
            1 => Some(args(&["doctor"])),
            2 => Some(args(&["node", "status"])),
            3 => Some(args(&["pool", "stats"])),
            4 => Some(args(&["wallet", "balance"])),
            5 => Some(args(&["mine", "status"])),
            _ => None,
        })
    }
}

fn node_menu() -> Result<Option<Vec<String>>> {
    {
        let items = [
            "Status",
            "Peers",
            "Last 10 blocks",
            "Custom block range",
            "Block by height/hash",
            "Transaction lookup",
            "Mempool",
            "Sync peers",
            "Raw JSON-RPC call",
            BACK,
        ];

        let Some(choice) = select("Node", &items)? else {
            return Ok(None);
        };

        match choice {
            0 => Ok(Some(args(&["node", "status"]))),
            1 => Ok(Some(args(&["node", "peers"]))),
            2 => Ok(Some(args(&["node", "blocks", "10"]))),
            3 => {
                let n = required_input("How many recent blocks?", Some("25"))?;
                Ok(Some(args_owned(vec!["node".into(), "blocks".into(), n])))
            }
            4 => {
                let id = required_input("Block height or hash", None)?;
                Ok(Some(args_owned(vec!["node".into(), "block".into(), id])))
            }
            5 => {
                let txid = required_input("Transaction ID", None)?;
                Ok(Some(args_owned(vec!["node".into(), "tx".into(), txid])))
            }
            6 => Ok(Some(args(&["node", "mempool"]))),
            7 => Ok(Some(args(&["node", "sync"]))),
            8 => {
                let method = required_input("RPC method", Some("getChainInfo"))?;
                let params = optional_input("Params JSON (blank = {})", Some("{}"))?;
                let params = if params.trim().is_empty() {
                    "{}".to_string()
                } else {
                    params
                };
                Ok(Some(args_owned(vec![
                    "node".into(),
                    "rpc".into(),
                    method,
                    params,
                ])))
            }
            _ => Ok(None),
        }
    }
}

fn pool_menu() -> Result<Option<Vec<String>>> {
    {
        let items = [
            "Stats",
            "Active miners",
            "Config",
            "Earnings for current wallet",
            "Earnings for custom address",
            BACK,
        ];
        let Some(choice) = select("Pool", &items)? else {
            return Ok(None);
        };

        match choice {
            0 => Ok(Some(args(&["pool", "stats"]))),
            1 => Ok(Some(args(&["pool", "miners"]))),
            2 => Ok(Some(args(&["pool", "config"]))),
            3 => Ok(Some(args(&["pool", "earnings"]))),
            4 => {
                let address = required_input("Wallet address", None)?;
                Ok(Some(args_owned(vec![
                    "pool".into(),
                    "earnings".into(),
                    "--address".into(),
                    address,
                ])))
            }
            _ => Ok(None),
        }
    }
}

fn mine_menu() -> Result<Option<Vec<String>>> {
    {
        let items = [
            "Start mining (quick default)",
            "Start mining (guided)",
            "Miner status",
            "CPU benchmark",
            "GPU benchmark",
            "Ekam benchmark",
            "Stop miner",
            "DCR status",
            "DCR start",
            "DCR stop",
            BACK,
        ];

        let Some(choice) = select("Mining", &items)? else {
            return Ok(None);
        };

        match choice {
            0 => Ok(Some(args(&["mine", "start"]))),
            1 => Ok(Some(guided_mine_start()?)),
            2 => Ok(Some(args(&["mine", "status"]))),
            3 => Ok(Some(args(&["mine", "bench"]))),
            4 => {
                let mut argv = args(&["mine", "bench", "--gpu"]);
                apply_backend_flag(&mut argv, choose_gpu_backend(false)?);
                apply_optional_flag(
                    &mut argv,
                    "--work-size",
                    optional_input("Work size (blank = default)", None)?,
                );
                apply_optional_flag(
                    &mut argv,
                    "--secs",
                    optional_input("Duration seconds (blank = 5)", Some("5"))?,
                );
                Ok(Some(argv))
            }
            5 => {
                let mut argv = args(&["mine", "bench", "--ekam"]);
                apply_backend_flag(&mut argv, choose_gpu_backend(true)?);
                apply_optional_flag(
                    &mut argv,
                    "--work-size",
                    optional_input("Work size (blank = default)", None)?,
                );
                apply_optional_flag(
                    &mut argv,
                    "--secs",
                    optional_input("Duration seconds (blank = 5)", Some("5"))?,
                );
                Ok(Some(argv))
            }
            6 => Ok(Some(args(&["mine", "stop"]))),
            7 => Ok(Some(args(&["mine", "dcr", "status"]))),
            8 => Ok(Some(args(&["mine", "dcr", "start"]))),
            9 => Ok(Some(args(&["mine", "dcr", "stop"]))),
            _ => Ok(None),
        }
    }
}

fn wallet_menu() -> Result<Option<Vec<String>>> {
    {
        let items = [
            "Current wallet address",
            "Current wallet balance",
            "Custom address balance",
            "Send ZION",
            "Generate new wallet",
            "Wallet info",
            "Import mnemonic",
            BACK,
        ];
        let Some(choice) = select("Wallet", &items)? else {
            return Ok(None);
        };

        match choice {
            0 => Ok(Some(args(&["wallet", "address"]))),
            1 => Ok(Some(args(&["wallet", "balance"]))),
            2 => {
                let address = required_input("Address", None)?;
                Ok(Some(args_owned(vec![
                    "wallet".into(),
                    "balance".into(),
                    "--address".into(),
                    address,
                ])))
            }
            3 => Ok(Some(guided_wallet_send()?)),
            4 => Ok(Some(args(&["wallet", "new", "--mnemonic"]))),
            5 => Ok(Some(args(&["wallet", "info"]))),
            6 => {
                let mnemonic = required_input("Mnemonic phrase", None)?;
                Ok(Some(args_owned(vec![
                    "wallet".into(),
                    "import-mnemonic".into(),
                    "--mnemonic".into(),
                    mnemonic,
                ])))
            }
            _ => Ok(None),
        }
    }
}

fn config_menu() -> Result<Option<Vec<String>>> {
    {
        let items = [
            "Show config",
            "Config path",
            "Validate",
            "Init wizard",
            "Set key/value",
            BACK,
        ];
        let Some(choice) = select("Config", &items)? else {
            return Ok(None);
        };

        match choice {
            0 => Ok(Some(args(&["config", "show"]))),
            1 => Ok(Some(args(&["config", "path"]))),
            2 => Ok(Some(args(&["config", "validate"]))),
            3 => Ok(Some(args(&["config", "init"]))),
            4 => {
                let key = required_input("Config key", None)?;
                let value = required_input("Config value", None)?;
                Ok(Some(args_owned(vec![
                    "config".into(),
                    "set".into(),
                    key,
                    value,
                ])))
            }
            _ => Ok(None),
        }
    }
}

fn guided_mine_start() -> Result<Vec<String>> {
    let mut argv = args(&["mine", "start"]);
    let backend = choose_backend()?;
    let profile = choose_profile()?;

    apply_optional_flag(
        &mut argv,
        "--pool",
        optional_input("Pool host:port (blank = config)", None)?,
    );
    apply_optional_flag(
        &mut argv,
        "--wallet",
        optional_input("Wallet override (blank = config)", None)?,
    );
    apply_optional_flag(
        &mut argv,
        "--threads",
        optional_input("Threads (blank = auto)", None)?,
    );
    apply_backend_flag(&mut argv, backend);
    argv.push("--profile".into());
    argv.push(profile.into());
    Ok(argv)
}

fn guided_wallet_send() -> Result<Vec<String>> {
    let to = required_input("Recipient address", None)?;
    let amount = required_input("Amount in ZION", None)?;
    let memo = optional_input("Memo (optional)", None)?;

    let mut argv = args_owned(vec![
        "wallet".into(),
        "send".into(),
        "--to".into(),
        to,
        "--amount".into(),
        amount,
    ]);
    apply_optional_flag(&mut argv, "--memo", memo);
    Ok(argv)
}

fn choose_backend() -> Result<Option<&'static str>> {
    let items = ["auto", "cpu", "metal", "opencl", "cuda"];
    let Some(choice) = select("Mining backend", &items)? else {
        return Ok(None);
    };
    Ok(match items[choice] {
        "auto" => None,
        other => Some(other),
    })
}

fn choose_gpu_backend(allow_auto: bool) -> Result<Option<&'static str>> {
    let items: Vec<&str> = if allow_auto {
        vec!["auto", "metal", "opencl", "cuda"]
    } else {
        vec!["auto", "gpu", "metal", "opencl", "cuda"]
    };
    let Some(choice) = select("GPU backend", &items)? else {
        return Ok(None);
    };
    Ok(match items[choice] {
        "auto" => None,
        other => Some(other),
    })
}

fn choose_profile() -> Result<&'static str> {
    let items = ["pool", "solo", "benchmark", "dual"];
    let Some(choice) = select("Mining profile", &items)? else {
        return Ok("pool");
    };
    Ok(items[choice])
}

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

fn apply_optional_flag(argv: &mut Vec<String>, flag: &str, value: String) {
    if !value.trim().is_empty() {
        argv.push(flag.into());
        argv.push(value);
    }
}

fn apply_backend_flag(argv: &mut Vec<String>, backend: Option<&str>) {
    if let Some(backend) = backend {
        argv.push("--backend".into());
        argv.push(backend.into());
    }
}

fn args(parts: &[&str]) -> Vec<String> {
    let mut argv = vec!["zion".to_string()];
    argv.extend(parts.iter().map(|part| (*part).to_string()));
    argv
}

fn args_owned(parts: Vec<String>) -> Vec<String> {
    let mut argv = vec!["zion".to_string()];
    argv.extend(parts);
    argv
}
