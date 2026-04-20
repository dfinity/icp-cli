use clap::Subcommand;

pub(crate) mod set;
pub(crate) mod show;

/// Commands to manage canister IDs
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Set(set::SetArgs),
    Show(show::ShowArgs),
}
