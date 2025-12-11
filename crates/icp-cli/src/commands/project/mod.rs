use clap::Subcommand;

pub(crate) mod show;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Outputs the project's effective yaml configuration.
    ///
    /// The effective yaml configuration includes:
    ///
    /// - implicit networks
    ///
    /// - implicit environments
    ///
    /// - processed recipes
    ///
    Show(show::ShowArgs),
}
