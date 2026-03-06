use clap::Subcommand;

pub(crate) mod build;

/// Candid encoding utilities
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Build(build::BuildArgs),
}
