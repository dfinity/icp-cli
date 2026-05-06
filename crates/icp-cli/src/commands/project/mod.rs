use clap::Subcommand;

pub(crate) mod bundle;
pub(crate) mod show;

/// Manage the current project
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Show(show::ShowArgs),
    Bundle(bundle::BundleArgs),
}
