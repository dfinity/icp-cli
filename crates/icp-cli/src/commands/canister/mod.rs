use clap::Subcommand;

pub(crate) mod binding_env_vars;
pub(crate) mod build;
pub(crate) mod call;
pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod info;
pub(crate) mod install;
pub(crate) mod list;
pub(crate) mod settings;
pub(crate) mod show;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;
pub(crate) mod sync;
pub(crate) mod top_up;

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    /// Build a canister
    Build(build::BuildArgs),

    /// Make a canister call
    Call(call::CallArgs),

    /// Create a canister on a network
    Create(create::CreateArgs),

    /// Delete a canister from a network
    Delete(delete::DeleteArgs),

    /// Display a canister's information
    Info(info::InfoArgs),

    /// Install a built WASM to a canister on a network
    Install(install::InstallArgs),

    /// List the canisters in an environment
    List(list::ListArgs),

    #[command(subcommand)]
    Settings(settings::Command),

    /// Show a canister's details
    Show(show::ShowArgs),

    /// Start a canister on a network
    Start(start::StartArgs),

    /// Show the status of a canister
    Status(status::StatusArgs),

    /// Stop a canister on a network
    Stop(stop::StopArgs),

    /// Synchronize a canister
    Sync(sync::SyncArgs),

    /// Top up a canister with cycles
    TopUp(top_up::TopUpArgs),
}
