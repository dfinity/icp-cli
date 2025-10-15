use clap::Subcommand;

pub(crate) mod show;

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Outputs the project's effective yaml configuration.
    Show(show::ShowArgs),
}
