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

/// Lists principals one per line below a label, indented two spaces past
/// `indent` — the indent of the label itself — so they nest the way the other
/// lists in `canister status` and `canister settings show` do. `noun` names what
/// each line holds, and stands in for the list when it is empty. Principals are
/// sorted so repeated calls print the same order.
fn format_principal_list(
    principals: impl IntoIterator<Item = String>,
    noun: &str,
    indent: &str,
) -> String {
    let mut principals: Vec<String> = principals.into_iter().collect();
    principals.sort();

    if principals.is_empty() {
        return format!("\n{indent}  {noun} list is empty");
    }

    principals
        .iter()
        .map(|principal| format!("\n{indent}  {noun}: {principal}"))
        .collect()
}

/// Renders a visibility setting for `canister status` and `canister settings show`.
///
/// The policy goes on the label's own line, with any allowed viewers listed
/// below it. `viewer` names what the setting grants — "log viewer", "status
/// viewer" — since a report carries one line per setting and the entries would
/// otherwise not say which they belong to.
pub(crate) fn format_visibility(visibility: &Visibility, viewer: &str, indent: &str) -> String {
    match visibility {
        Visibility::Controllers => "Controllers".to_string(),
        Visibility::Public => "Public".to_string(),
        Visibility::AllowedViewers(viewers) => format!(
            "Allowed viewers{}",
            format_principal_list(viewers.iter().map(|p| p.to_string()), viewer, indent)
        ),
    }
}

/// Renders a canister's controllers for the same two reports. Unlike a
/// visibility setting, the list stands alone rather than qualifying a policy, so
/// the label is part of what this returns.
pub(crate) fn format_controllers(
    controllers: impl IntoIterator<Item = String>,
    indent: &str,
) -> String {
    format!(
        "{indent}Controllers:{}",
        format_principal_list(controllers, "controller", indent)
    )
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
            format!(
                "Status visibility: {}",
                format_visibility(&viewers, "status viewer", "")
            ),
            "Status visibility: Allowed viewers\n  \
             status viewer: aaaaa-aa\n  \
             status viewer: ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
        // `canister status`, where it sits two spaces in.
        assert_eq!(
            format!(
                "  Log visibility: {}",
                format_visibility(&viewers, "log viewer", "  ")
            ),
            "  Log visibility: Allowed viewers\n    \
             log viewer: aaaaa-aa\n    \
             log viewer: ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
    }

    /// The fixed policies stay on the label's line, and an empty list says so
    /// where its entries would have gone.
    #[test]
    fn fixed_policies_are_rendered_inline() {
        assert_eq!(
            format_visibility(&Visibility::Controllers, "log viewer", "  "),
            "Controllers"
        );
        assert_eq!(
            format_visibility(&Visibility::Public, "log viewer", "  "),
            "Public"
        );
        assert_eq!(
            format_visibility(&Visibility::AllowedViewers(vec![]), "log viewer", "  "),
            "Allowed viewers\n    log viewer list is empty"
        );
    }

    /// Controllers are listed the same way, but carry their own label.
    #[test]
    fn controllers_are_listed_one_per_line() {
        let controllers = ["ryjl3-tyaaa-aaaaa-aaaba-cai", "aaaaa-aa"].map(str::to_string);

        assert_eq!(
            format_controllers(controllers.clone(), "  "),
            "  Controllers:\n    \
             controller: aaaaa-aa\n    \
             controller: ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
        assert_eq!(
            format_controllers(controllers, ""),
            "Controllers:\n  \
             controller: aaaaa-aa\n  \
             controller: ryjl3-tyaaa-aaaaa-aaaba-cai"
        );
        assert_eq!(
            format_controllers([], "  "),
            "  Controllers:\n    controller list is empty"
        );
    }
}
