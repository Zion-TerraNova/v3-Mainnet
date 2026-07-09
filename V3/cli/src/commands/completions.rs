use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use std::io;

/// Generate shell completion script and print to stdout.
/// Usage: eval "$(zion completions zsh)"
pub fn run(shell: Shell) -> Result<()> {
    let mut cmd = crate::Cli::command();
    generate(shell, &mut cmd, "zion", &mut io::stdout());
    Ok(())
}
