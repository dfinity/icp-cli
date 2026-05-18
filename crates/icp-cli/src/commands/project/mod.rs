use clap::Subcommand;

pub(crate) mod show;

/// Display information about the current project
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Show(show::ShowArgs),
}
