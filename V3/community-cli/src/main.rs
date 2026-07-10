use anyhow::Result;
use clap::{CommandFactory, Parser};
use std::io::{self, IsTerminal};

use zion_public::commands::{ai, doctor, mine, monitor, node, pool, status, wallet};
use zion_public::config;
use zion_public::menu;
use zion_public::ui;
use zion_public::{Cli, Commands, ConfigCmd};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let should_open_menu = cli
        .command
        .as_ref()
        .map(|cmd| matches!(cmd, Commands::Menu))
        .unwrap_or(true);

    if should_open_menu {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            return Ok(());
        }
        return run_menu_session(cli.config).await;
    }

    dispatch(cli).await
}

async fn run_menu_session(default_config: Option<String>) -> Result<()> {
    let mut show_genesis = true;

    loop {
        // Load config fresh each time so stats reflect any changes.
        let cfg = config::load(default_config.as_deref())?;

        let args = match menu::run(show_genesis, &cfg).await? {
            Some(args) => args,
            None => return Ok(()),
        };
        show_genesis = false;

        let mut cli = match Cli::try_parse_from(args) {
            Ok(c) => c,
            Err(e) => {
                e.print()?;
                continue;
            }
        };
        if cli.config.is_none() {
            cli.config = default_config.clone();
        }

        // Menu command = re-open menu (used by guided setup "skip")
        if matches!(cli.command, Some(Commands::Menu)) {
            continue;
        }

        if let Err(err) = dispatch(cli).await {
            ui::print_err(&format!("{}", err));
        }

        ui::wait_for_enter("Press Enter to return to the menu...")?;
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    let cfg = config::load(cli.config.as_deref())?;

    let command = cli
        .command
        .ok_or_else(|| anyhow::anyhow!("no command selected"))?;

    match command {
        Commands::Menu => unreachable!("interactive menu is resolved before dispatch"),
        Commands::Version => {
            ui::print_header("ZION Public CLI");
            ui::print_row("Version", env!("CARGO_PKG_VERSION"));
            ui::print_row("Edition", "Public — community release");
            ui::print_row("Homepage", env!("CARGO_PKG_HOMEPAGE"));
            println!();
            Ok(())
        }
        Commands::Wallet { cmd } => wallet::run(&cfg, cmd).await,
        Commands::Node { cmd } => node::run(&cfg, cmd).await,
        Commands::Mine { cmd } => mine::run(&cfg, cmd).await,
        Commands::Pool { cmd } => pool::run(&cfg, cmd).await,
        Commands::Ai { cmd } => ai::run(&cfg, cmd).await,
        Commands::Status => status::run(&cfg).await,
        Commands::Doctor => doctor::run(&cfg).await,
        Commands::Monitor => monitor::run(&cfg).await,
        Commands::Config { cmd } => match cmd {
            ConfigCmd::Set { key, value } => config::set_value(&key, &value),
            ConfigCmd::Path => {
                let path = config::config_path()?;
                println!("{}", path.display());
                Ok(())
            }
        },
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "zion", &mut io::stdout());
            Ok(())
        }
    }
}
