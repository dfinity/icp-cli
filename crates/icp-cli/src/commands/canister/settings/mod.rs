use clap::Subcommand;

pub(crate) mod show;
pub(crate) mod sync;
pub(crate) mod update;

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    /// Show the status of a canister. By default this queries the
    /// status endpoint of the management canister. If the caller is not
    /// a controller, falls back on fetching public information from the
    /// state tree.
    Show(show::ShowArgs),
    /// Change a canister's settings to specified values
    Update(update::UpdateArgs),
    /// Synchronize a canister's settings with those defined in the project
    Sync(sync::SyncArgs),
}
