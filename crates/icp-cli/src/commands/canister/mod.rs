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
///
/// The policy goes on the label's own line; allowed viewers are listed one per
/// line below it, indented two spaces past `indent` — the indent of the label
/// itself — so they nest the way the other lists in those reports do. Viewers
/// are sorted so repeated calls print the same order.
pub(crate) fn format_visibility(visibility: &Visibility, indent: &str) -> String {
    match visibility {
        Visibility::Controllers => "Controllers".to_string(),
        Visibility::Public => "Public".to_string(),
        Visibility::AllowedViewers(viewers) if viewers.is_empty() => {
            "Allowed viewers list is empty".to_string()
        }
        Visibility::AllowedViewers(viewers) => {
            let mut viewers: Vec<String> = viewers.iter().map(|p| p.to_string()).collect();
            viewers.sort();
            let mut out = "Allowed viewers".to_string();
            for viewer in viewers {
                out.push_str(&format!("\n{indent}  viewer: {viewer}"));
            }
            out
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

#[cfg(test)]
mod tests {
    use candid::Principal;

    use super::*;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    /// Allowed viewers are listed one per line, sorted, and nested two spaces
    /// past the label — which sits at a different indent in `canister status`
    /// than in `canister settings show`.
    #[test]
    fn allowed_viewers_are_listed_one_per_line() {
        let viewers = Visibility::AllowedViewers(vec![
            principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
            principal("aaaaa-aa"),
        ]);

        // `settings show`, where the label is not indented.
        assert_eq!(
            format!("Status visibility: {}", format_visibility(&viewers, "")),
            "Status visibility: Allowed viewers\n  \
             viewer: aaaaa-aa\n  \
             viewer: ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
        // `canister status`, where it sits two spaces in.
        assert_eq!(
            format!("  Status visibility: {}", format_visibility(&viewers, "  ")),
            "  Status visibility: Allowed viewers\n    \
             viewer: aaaaa-aa\n    \
             viewer: ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
    }

    /// The other policies stay on the label's line.
    #[test]
    fn fixed_policies_are_rendered_inline() {
        assert_eq!(
            format_visibility(&Visibility::Controllers, "  "),
            "Controllers"
        );
        assert_eq!(format_visibility(&Visibility::Public, "  "), "Public");
        assert_eq!(
            format_visibility(&Visibility::AllowedViewers(vec![]), "  "),
            "Allowed viewers list is empty"
        );
    }
}
