use anyhow::Result;
use clap::{CommandFactory, Parser};
use std::io::{self, IsTerminal};

use zion_cli::commands::{
    completions, doctor, mine, node, onboard, pool, status, update, wallet,
};
use zion_cli::config;
use zion_cli::menu;
use zion_cli::ui;
use zion_cli::{Cli, Commands, ConfigCmd};

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
        let args = match menu::run(show_genesis)? {
            Some(args) => args,
            None => return Ok(()),
        };
        show_genesis = false;

        let mut cli = Cli::try_parse_from(args)?;
        if cli.config.is_none() {
            cli.config = default_config.clone();
        }

        if let Err(err) = dispatch(cli).await {
            ui::print_err(&format!("{}", err));
            println!();
        }

        ui::wait_for_enter("Press Enter to return to the ZION menu...")?;
    }
}

async fn dispatch(cli: Cli) -> Result<()> {
    let cfg = config::load(cli.config.as_deref())?;

    // Auto-check for updates in background (silent)
    if cfg.cli.auto_update_check {
        let update_cfg = cfg.clone();
        tokio::spawn(async move {
            let _ = update::run_with_auto_check(&update_cfg, true, false, true).await;
        });
    }

    let command = cli
        .command
        .ok_or_else(|| anyhow::anyhow!("no command selected"))?;

    match command {
        Commands::Menu => unreachable!("interactive menu is resolved before dispatch"),
        Commands::Version => update::print_version_surface(&cfg),
        Commands::Update { check, yes } => update::run(&cfg, check, yes).await,
        Commands::Onboard => onboard::run(&cfg).await,
        Commands::Status => status::run(&cfg).await,
        Commands::Doctor => doctor::run(&cfg).await,
        Commands::Node { cmd } => node::run(&cfg, cmd).await,
        Commands::Pool { cmd } => pool::run(&cfg, cmd).await,
        Commands::Mine { cmd } => mine::run(&cfg, cmd).await,
        Commands::Wallet { cmd } => wallet::run(&cfg, cmd).await,
        Commands::Completions { shell } => completions::run(shell),
        Commands::Config { cmd } => match cmd {
            ConfigCmd::Show => {
                let text = toml::to_string_pretty(&cfg)?;
                println!("{}", text);
                Ok(())
            }
            ConfigCmd::Path => {
                println!("{}", config::config_path()?.display());
                Ok(())
            }
            ConfigCmd::Set { key, value } => config::set_value(&key, &value),
            ConfigCmd::Validate => {
                ui::print_header("Config Validation");
                let report = config::validate(&cfg);

                for warning in &report.warnings {
                    ui::print_warn(warning);
                }
                for error in &report.errors {
                    ui::print_err(error);
                }

                if report.is_ok() {
                    ui::print_ok("Config is valid");
                    Ok(())
                } else {
                    anyhow::bail!(
                        "Config validation failed with {} error(s)",
                        report.errors.len()
                    )
                }
            }
            ConfigCmd::Init => onboard::run(&cfg).await,
        },
    }
}
