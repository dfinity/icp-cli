use clap::Subcommand;

pub(crate) mod hsm;
pub(crate) mod ii;

/// Link an external key to a new identity
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Hsm(hsm::HsmArgs),
    #[command(hide = true)]
    Ii(ii::IiArgs),
}
