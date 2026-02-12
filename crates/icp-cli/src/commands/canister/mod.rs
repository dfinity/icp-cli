use clap::Subcommand;

pub(crate) mod call;
pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod install;
pub(crate) mod list;
pub(crate) mod metadata;
pub(crate) mod migrate_id;
pub(crate) mod settings;
pub(crate) mod snapshot;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;
pub(crate) mod top_up;

/// Perform canister operations against a network
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    Call(call::CallArgs),
    Create(create::CreateArgs),
    Delete(delete::DeleteArgs),
    Install(install::InstallArgs),
    List(list::ListArgs),
    Metadata(metadata::MetadataArgs),
    MigrateId(migrate_id::MigrateIdArgs),
    #[command(subcommand)]
    Settings(settings::Command),
    #[command(subcommand)]
    Snapshot(snapshot::Command),
    Start(start::StartArgs),
    Status(status::StatusArgs),
    Stop(stop::StopArgs),
    TopUp(top_up::TopUpArgs),
}
