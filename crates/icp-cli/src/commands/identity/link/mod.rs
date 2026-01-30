use clap::Subcommand;

pub(crate) mod hsm;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Link an HSM key to a new identity
    Hsm(hsm::HsmArgs),
}
