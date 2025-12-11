use clap::Subcommand;

pub(crate) mod call;
pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod install;
pub(crate) mod list;
pub(crate) mod settings;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;
pub(crate) mod top_up;

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    /// Make a canister call
    Call(call::CallArgs),

    /// Create a canister on a network
    Create(create::CreateArgs),

    /// Delete a canister from a network
    Delete(delete::DeleteArgs),

    /// Install a built WASM to a canister on a network
    Install(install::InstallArgs),

    /// List the canisters in an environment
    List(list::ListArgs),

    /// Commands to manage canister settings
    #[command(subcommand)]
    Settings(settings::Command),

    /// Start a canister on a network
    Start(start::StartArgs),

    /// Show the status of canister(s).
    ///
    /// By default this queries the status endpoint of the management canister.
    /// If the caller is not a controller, falls back on fetching public
    /// information from the state tree.
    Status(status::StatusArgs),

    /// Stop a canister on a network
    Stop(stop::StopArgs),

    /// Top up a canister with cycles
    TopUp(top_up::TopUpArgs),
}
