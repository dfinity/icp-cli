use clap::Subcommand;

pub(crate) mod list;

/// Show information about the current project environments
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    List(list::ListArgs),
}
