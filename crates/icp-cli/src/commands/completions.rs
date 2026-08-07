use std::io;

use clap::{Args, CommandFactory};
use clap_complete::{Shell, generate};

use crate::Cli;

/// Generate a shell completion script
///
/// The script is written to stdout; redirect it to the location your shell
/// loads completions from, or source it directly from your shell profile.
#[derive(Debug, Args)]
#[command(after_long_help = "\
Examples:

    # Bash
    icp completions bash > /etc/bash_completion.d/icp

    # Zsh, into a directory on your $fpath
    icp completions zsh > ~/.zfunc/_icp

    # Fish
    icp completions fish > ~/.config/fish/completions/icp.fish

    # PowerShell, appended to your profile
    icp completions powershell >> $PROFILE
")]
pub(crate) struct CompletionsArgs {
    /// The shell to generate a completion script for
    shell: Shell,
}

pub(crate) fn exec(args: &CompletionsArgs) {
    let mut command = Cli::command();
    generate(args.shell, &mut command, "icp", &mut io::stdout());
}
