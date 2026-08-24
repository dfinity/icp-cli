use clap::Subcommand;
use icp::canister::Visibility;

pub(crate) mod call;
pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod install;
pub(crate) mod link;
pub(crate) mod list;
pub(crate) mod logs;
pub(crate) mod metadata;
pub(crate) mod migrate_id;
pub(crate) mod settings;
pub(crate) mod snapshot;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;
pub(crate) mod top_up;

/// Renders a visibility setting for `canister status` and `canister settings show`.
/// Viewers are sorted so repeated calls print the same order.
pub(crate) fn format_visibility(visibility: &Visibility) -> String {
    match visibility {
        Visibility::Controllers => "Controllers".to_string(),
        Visibility::Public => "Public".to_string(),
        Visibility::AllowedViewers(viewers) if viewers.is_empty() => {
            "Allowed viewers list is empty".to_string()
        }
        Visibility::AllowedViewers(viewers) => {
            let mut viewers: Vec<String> = viewers.iter().map(|p| p.to_string()).collect();
            viewers.sort();
            format!("Allowed viewers: {}", viewers.join(", "))
        }
    }
}

/// Perform canister operations against a network
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    Call(call::CallArgs),
    Create(create::CreateArgs),
    Delete(delete::DeleteArgs),
    Install(install::InstallArgs),
    Link(link::LinkArgs),
    List(list::ListArgs),
    Logs(logs::LogsArgs),
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
