use clap::Subcommand;

pub(crate) mod hsm;

/// Link an external key to a new identity
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Hsm(hsm::HsmArgs),
}
