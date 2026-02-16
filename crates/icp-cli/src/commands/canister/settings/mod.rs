use clap::Subcommand;

pub(crate) mod show;
pub(crate) mod sync;
pub(crate) mod update;

/// Commands to manage canister settings
#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    Show(show::ShowArgs),
    Update(update::UpdateArgs),
    Sync(sync::SyncArgs),
}
