use anyhow::bail;
use candid::Nat;
use clap::{ArgAction, Args};
use dialoguer::Confirm;
use ic_agent::Identity;
use ic_agent::export::Principal;
use ic_management_canister_types::{
    CanisterIdRecord, CanisterSettings, CanisterStatusResult, EnvironmentVariable,
    UpdateSettingsArgs,
};
use icp::ProjectLoadError;
use icp::canister::Visibility;
use icp::context::{CanisterSelection, Context};
use icp::parsers::{CyclesAmount, DurationAmount, MemoryAmount};
use std::collections::{HashMap, HashSet};
use tracing::warn;

use crate::commands::args;
use icp::operations::proxy_management;

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct ControllerOpt {
    /// Add one or more principals to the canister's controller list.
    #[arg(long, action = ArgAction::Append)]
    add_controller: Option<Vec<Principal>>,

    /// Remove one or more principals from the canister's controller list.
    ///
    /// Warning: Removing yourself will cause you to lose control of the canister.
    #[arg(long, action = ArgAction::Append)]
    remove_controller: Option<Vec<Principal>>,

    /// Remove all controllers.
    ///
    /// Warning: This will cause you to lose control of the canister, unless you
    /// add your user principal back in `--add-controller` in the same command.
    #[arg(long, conflicts_with = "remove_controller")]
    remove_all_controllers: bool,
}

impl ControllerOpt {
    pub(crate) fn require_current_settings(&self) -> bool {
        !self.remove_all_controllers
            && (self.add_controller.is_some() || self.remove_controller.is_some())
    }
}

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct LogVisibilityOpt {
    /// Set log visibility to a fixed policy [possible values: controllers, public].
    /// Conflicts with --add-log-viewer, --remove-log-viewer, and --set-log-viewer.
    /// Use --add-log-viewer / --set-log-viewer to grant access to specific principals instead.
    #[arg(
        long,
        value_parser = visibility_parser,
        conflicts_with("add_log_viewer"),
        conflicts_with("remove_log_viewer"),
        conflicts_with("set_log_viewer"),
    )]
    log_visibility: Option<Visibility>,

    /// Add a principal to the allowed log viewers list.
    ///
    /// Rejected while log visibility is public, which has no viewers list to
    /// add to; use --set-log-viewer to replace the public policy with a list.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_log_viewer"))]
    add_log_viewer: Option<Vec<Principal>>,

    /// Remove a principal from the allowed log viewers list.
    ///
    /// Rejected while log visibility is public, which has no viewers list to
    /// remove from; use --log-visibility controllers to revoke public access.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_log_viewer"))]
    remove_log_viewer: Option<Vec<Principal>>,

    /// Replace the allowed log viewers list with the specified principals
    #[arg(long, action = ArgAction::Append)]
    set_log_viewer: Option<Vec<Principal>>,
}

impl LogVisibilityOpt {
    fn flags(&self) -> VisibilityFlags<'_> {
        VisibilityFlags {
            label: "Log visibility",
            stem: "log",
            fixed: self.log_visibility.as_ref(),
            add: self.add_log_viewer.as_deref(),
            remove: self.remove_log_viewer.as_deref(),
            set: self.set_log_viewer.as_deref(),
        }
    }

    pub(crate) fn require_current_settings(&self) -> bool {
        self.flags().require_current_settings()
    }
}

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct SnapshotVisibilityOpt {
    /// Set snapshot visibility to a fixed policy [possible values: controllers, public].
    /// Conflicts with --add-snapshot-viewer, --remove-snapshot-viewer, and --set-snapshot-viewer.
    /// Use --add-snapshot-viewer / --set-snapshot-viewer to grant access to specific principals instead.
    #[arg(
        long,
        value_parser = visibility_parser,
        conflicts_with("add_snapshot_viewer"),
        conflicts_with("remove_snapshot_viewer"),
        conflicts_with("set_snapshot_viewer"),
    )]
    snapshot_visibility: Option<Visibility>,

    /// Add a principal to the allowed snapshot viewers list.
    ///
    /// Rejected while snapshot visibility is public, which has no viewers list to
    /// add to; use --set-snapshot-viewer to replace the public policy with a list.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_snapshot_viewer"))]
    add_snapshot_viewer: Option<Vec<Principal>>,

    /// Remove a principal from the allowed snapshot viewers list.
    ///
    /// Rejected while snapshot visibility is public, which has no viewers list to
    /// remove from; use --snapshot-visibility controllers to revoke public access.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_snapshot_viewer"))]
    remove_snapshot_viewer: Option<Vec<Principal>>,

    /// Replace the allowed snapshot viewers list with the specified principals
    #[arg(long, action = ArgAction::Append)]
    set_snapshot_viewer: Option<Vec<Principal>>,
}

impl SnapshotVisibilityOpt {
    fn flags(&self) -> VisibilityFlags<'_> {
        VisibilityFlags {
            label: "Snapshot visibility",
            stem: "snapshot",
            fixed: self.snapshot_visibility.as_ref(),
            add: self.add_snapshot_viewer.as_deref(),
            remove: self.remove_snapshot_viewer.as_deref(),
            set: self.set_snapshot_viewer.as_deref(),
        }
    }

    pub(crate) fn require_current_settings(&self) -> bool {
        self.flags().require_current_settings()
    }
}

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct StatusVisibilityOpt {
    /// Set status visibility to a fixed policy [possible values: controllers, public].
    /// Conflicts with --add-status-viewer, --remove-status-viewer, and --set-status-viewer.
    /// Use --add-status-viewer / --set-status-viewer to grant access to specific principals instead.
    #[arg(
        long,
        value_parser = visibility_parser,
        conflicts_with("add_status_viewer"),
        conflicts_with("remove_status_viewer"),
        conflicts_with("set_status_viewer"),
    )]
    status_visibility: Option<Visibility>,

    /// Add a principal to the allowed status viewers list.
    ///
    /// Rejected while status visibility is public, which has no viewers list to
    /// add to; use --set-status-viewer to replace the public policy with a list.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_status_viewer"))]
    add_status_viewer: Option<Vec<Principal>>,

    /// Remove a principal from the allowed status viewers list.
    ///
    /// Rejected while status visibility is public, which has no viewers list to
    /// remove from; use --status-visibility controllers to revoke public access.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_status_viewer"))]
    remove_status_viewer: Option<Vec<Principal>>,

    /// Replace the allowed status viewers list with the specified principals
    #[arg(long, action = ArgAction::Append)]
    set_status_viewer: Option<Vec<Principal>>,
}

impl StatusVisibilityOpt {
    fn flags(&self) -> VisibilityFlags<'_> {
        VisibilityFlags {
            label: "Status visibility",
            stem: "status",
            fixed: self.status_visibility.as_ref(),
            add: self.add_status_viewer.as_deref(),
            remove: self.remove_status_viewer.as_deref(),
            set: self.set_status_viewer.as_deref(),
        }
    }

    pub(crate) fn require_current_settings(&self) -> bool {
        self.flags().require_current_settings()
    }
}

/// The flags of one visibility group, borrowed so the resolution below is
/// written once for every setting that has such a group.
struct VisibilityFlags<'a> {
    /// How the setting reads in prose, e.g. `Log visibility`.
    label: &'static str,
    /// The stem its flags share, e.g. `log` for `--add-log-viewer`.
    stem: &'static str,
    fixed: Option<&'a Visibility>,
    add: Option<&'a [Principal]>,
    remove: Option<&'a [Principal]>,
    set: Option<&'a [Principal]>,
}

impl VisibilityFlags<'_> {
    /// Any viewer edit is resolved against the canister's current policy, so it
    /// has to be fetched first: `--add` and `--remove` build on the current
    /// list, and `--set` needs it only to warn about what it replaces.
    fn require_current_settings(&self) -> bool {
        self.add.is_some() || self.remove.is_some() || self.set.is_some()
    }

    /// Turns the flags of one group into the policy to send, rejecting the
    /// edits the group cannot express. Pure: it neither reads the network nor
    /// prints, and what a legal edit costs the caller is warned about by
    /// [`maybe_warn_on_lost_access`].
    fn resolve(&self, current: Option<&Visibility>) -> Result<Visibility, anyhow::Error> {
        if let Some(fixed) = self.fixed {
            return Ok(fixed.clone());
        }

        if let Some(viewers) = self.set {
            return Ok(Visibility::AllowedViewers(viewers.to_vec()));
        }

        let mut viewers = match current {
            Some(Visibility::AllowedViewers(viewers)) => viewers.clone(),
            Some(Visibility::Public) if self.add.is_some() || self.remove.is_some() => {
                return Err(self.public_has_no_viewers_list());
            }
            // `controllers` has no list either, but building one from it only
            // ever grants access on top, so the edits mean what they say.
            _ => vec![],
        };

        if let Some(to_be_added) = self.add {
            for principal in to_be_added {
                if !viewers.contains(principal) {
                    viewers.push(*principal);
                }
            }
        }

        if let Some(to_be_removed) = self.remove {
            viewers.retain(|principal| !to_be_removed.contains(principal));
        }

        Ok(Visibility::AllowedViewers(viewers))
    }

    /// `public` grants access to everyone, so it carries no viewers list for a
    /// relative edit to be relative to. The only way to honour one would be to
    /// start a list from empty, which revokes everyone else's access — a policy
    /// change these flags do not name, so it is refused in favour of the flags
    /// that do.
    fn public_has_no_viewers_list(&self) -> anyhow::Error {
        let (label, stem) = (self.label, self.stem);
        let edits = match (self.add.is_some(), self.remove.is_some()) {
            (true, true) => format!("--add-{stem}-viewer / --remove-{stem}-viewer"),
            (true, false) => format!("--add-{stem}-viewer"),
            _ => format!("--remove-{stem}-viewer"),
        };

        anyhow::anyhow!(
            "{label} is currently public, so there is no allowed viewers list for {edits} to edit. \
             Use `--set-{stem}-viewer <PRINCIPAL>` to replace the public policy with an explicit \
             list, or `--{stem}-visibility controllers` to revoke public access on its own."
        )
    }
}

/// Warns when a legal viewer edit still takes access away rather than granting
/// it: `--set-*-viewer` on a `public` canister revokes everyone else's access,
/// and removing the last viewer leaves the controllers alone with it.
///
/// A warning rather than a prompt or a refusal: both edits state outright what
/// the new list is, so unlike the relative edits [`VisibilityFlags::resolve`]
/// refuses on a `public` canister, they say what they do. Either is reversible
/// by any controller, and a prompt would break scripted use.
fn maybe_warn_on_lost_access(label: &str, current: Option<&Visibility>, resolved: &Visibility) {
    let Visibility::AllowedViewers(viewers) = resolved else {
        return;
    };

    if current == Some(&Visibility::Public) {
        warn!(
            "{label} is currently public; listing allowed viewers revokes access for everyone else"
        );
    }
    if viewers.is_empty() {
        warn!("{label} is left with no allowed viewers; only the controllers keep access");
    }
}

/// Resolves one visibility group against the policy the canister carries now,
/// warning about what the result costs.
fn resolve_visibility(
    flags: VisibilityFlags<'_>,
    current: Option<Visibility>,
) -> Result<Visibility, anyhow::Error> {
    let resolved = flags.resolve(current.as_ref())?;
    maybe_warn_on_lost_access(flags.label, current.as_ref(), &resolved);
    Ok(resolved)
}

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct EnvironmentVariableOpt {
    /// Add a canister environment variable in KEY=VALUE format
    #[arg(long, value_parser = environment_variable_parser, action = ArgAction::Append)]
    add_environment_variable: Option<Vec<EnvironmentVariable>>,

    /// Remove a canister environment variable by key name
    #[arg(long, action = ArgAction::Append)]
    remove_environment_variable: Option<Vec<String>>,
}

impl EnvironmentVariableOpt {
    pub(crate) fn require_current_settings(&self) -> bool {
        self.add_environment_variable.is_some() || self.remove_environment_variable.is_some()
    }
}

/// Change a canister's settings to specified values
#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Force the operation without confirmation prompts
    #[arg(short = 'f', long)]
    force: bool,

    #[command(flatten)]
    controllers: Option<ControllerOpt>,

    /// Compute allocation percentage (0-100). Represents a guaranteed share of a subnet's compute capacity.
    #[arg(long, value_parser = compute_allocation_parser)]
    compute_allocation: Option<u8>,

    /// Memory allocation in bytes. Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    #[arg(long)]
    memory_allocation: Option<MemoryAmount>,

    /// Freezing threshold. Controls how long a canister can be inactive before being frozen.
    /// Supports duration suffixes: s (seconds), m (minutes), h (hours), d (days), w (weeks).
    /// A bare number is treated as seconds.
    #[arg(long)]
    freezing_threshold: Option<DurationAmount>,

    /// Upper limit on cycles reserved for future resource payments.
    /// Memory allocations that would push the reserved balance above this limit will fail.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long)]
    reserved_cycles_limit: Option<CyclesAmount>,

    /// Wasm memory limit in bytes. Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    #[arg(long)]
    wasm_memory_limit: Option<MemoryAmount>,

    /// Wasm memory threshold in bytes. Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    #[arg(long)]
    wasm_memory_threshold: Option<MemoryAmount>,

    /// Log memory limit in bytes (max 2 MiB). Oldest logs are purged when usage exceeds this value.
    /// Supports suffixes: kb, kib, mb, mib (e.g. "2mib" or "256kib"). Canister default is 4096 bytes.
    #[arg(long)]
    log_memory_limit: Option<MemoryAmount>,

    #[command(flatten)]
    log_visibility: Option<LogVisibilityOpt>,

    #[command(flatten)]
    snapshot_visibility: Option<SnapshotVisibilityOpt>,

    #[command(flatten)]
    status_visibility: Option<StatusVisibilityOpt>,

    #[command(flatten)]
    environment_variables: Option<EnvironmentVariableOpt>,

    /// Principal of a proxy canister to route the management canister calls through.
    #[arg(long)]
    proxy: Option<Principal>,
}

pub(crate) async fn exec(ctx: &Context, args: &UpdateArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let identity = ctx.get_identity(&selections.identity, None).await?;
    let caller_principal = identity
        .sender()
        .map_err(|e| anyhow::anyhow!("failed to get caller principal: {e}"))?;

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let configured_settings = if let CanisterSelection::Named(name) = &selections.canister {
        match ctx.project.load().await {
            Ok(p) => p.canisters[name].1.settings.clone(),
            Err(ProjectLoadError::Locate { .. }) => <_>::default(),
            Err(e) => bail!("failed to load project: {}", e),
        }
    } else {
        <_>::default()
    };

    let mut current_status: Option<CanisterStatusResult> = None;
    if require_current_settings(args) {
        current_status = Some(
            proxy_management::canister_status(
                &agent,
                args.proxy,
                CanisterIdRecord { canister_id: cid },
            )
            .await?,
        );
    }

    // TODO(VZ): Ask for consent if the freezing threshold is too long or too short.

    // Handle controllers.
    let mut controllers: Option<Vec<Principal>> = None;
    if let Some(controllers_opt) = &args.controllers {
        controllers = get_controllers(controllers_opt, current_status.as_ref());

        // Check if the effective controller is being removed from the controller list.
        // When --proxy is set, the proxy canister is the one making management calls and
        // is the effective controller. Without --proxy, it's the caller's identity.
        let effective_controller = args.proxy.unwrap_or(caller_principal);
        if let Some(new_controllers) = &controllers
            && !new_controllers.contains(&effective_controller)
            && !args.force
        {
            if args.proxy.is_some() {
                warn!(
                    "You are about to remove the proxy canister ({effective_controller}) from the controllers list."
                );
                warn!(
                    "This will prevent further management calls through this proxy and cannot be undone."
                );
            } else {
                warn!("You are about to remove yourself from the controllers list.");
                warn!("This will cause you to lose control of the canister and cannot be undone.");
            }

            let confirmed = Confirm::new()
                .with_prompt("Do you want to proceed?")
                .default(false)
                .interact()?;

            if !confirmed {
                bail!("Operation cancelled by user");
            }
        }
    }

    // Handle log, snapshot and status visibility.

    let log_visibility = args
        .log_visibility
        .as_ref()
        .map(|opt| {
            let current = current_status
                .as_ref()
                .map(|status| Visibility::from(status.settings.log_visibility.clone()));
            resolve_visibility(opt.flags(), current)
        })
        .transpose()?;
    let snapshot_visibility = args
        .snapshot_visibility
        .as_ref()
        .map(|opt| {
            let current = current_status
                .as_ref()
                .map(|status| Visibility::from(status.settings.snapshot_visibility.clone()));
            resolve_visibility(opt.flags(), current)
        })
        .transpose()?;
    let status_visibility = args
        .status_visibility
        .as_ref()
        .map(|opt| {
            let current = current_status
                .as_ref()
                .map(|status| Visibility::from(status.settings.status_visibility.clone()));
            resolve_visibility(opt.flags(), current)
        })
        .transpose()?;

    // Handle environment variables.
    let mut environment_variables: Option<Vec<EnvironmentVariable>> = None;
    if let Some(environment_variables_opt) = &args.environment_variables {
        maybe_warn_on_env_vars_change(&configured_settings, environment_variables_opt);
        environment_variables =
            get_environment_variables(environment_variables_opt, current_status.as_ref());
    }

    // Build settings with warnings for configured values
    if args.compute_allocation.is_some() && configured_settings.compute_allocation.is_some() {
        warn!(
            "Compute allocation is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if args.memory_allocation.is_some() && configured_settings.memory_allocation.is_some() {
        warn!(
            "Memory allocation is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if args.freezing_threshold.is_some() && configured_settings.freezing_threshold.is_some() {
        warn!(
            "Freezing threshold is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if args.reserved_cycles_limit.is_some() && configured_settings.reserved_cycles_limit.is_some() {
        warn!(
            "Reserved cycles limit is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if args.wasm_memory_limit.is_some() && configured_settings.wasm_memory_limit.is_some() {
        warn!(
            "Wasm memory limit is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if args.wasm_memory_threshold.is_some() && configured_settings.wasm_memory_threshold.is_some() {
        warn!(
            "Wasm memory threshold is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if args.log_memory_limit.is_some() && configured_settings.log_memory_limit.is_some() {
        warn!(
            "Log memory limit is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if log_visibility.is_some() && configured_settings.log_visibility.is_some() {
        warn!(
            "Log visibility is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if snapshot_visibility.is_some() && configured_settings.snapshot_visibility.is_some() {
        warn!(
            "Snapshot visibility is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }
    if status_visibility.is_some() && configured_settings.status_visibility.is_some() {
        warn!(
            "Status visibility is already set in icp.yaml; this new value will be overridden on next settings sync"
        );
    }

    let settings = CanisterSettings {
        controllers,
        compute_allocation: args.compute_allocation.map(|v| Nat::from(v as u64)),
        memory_allocation: args.memory_allocation.as_ref().map(|m| Nat::from(m.get())),
        freezing_threshold: args.freezing_threshold.as_ref().map(|d| Nat::from(d.get())),
        reserved_cycles_limit: args
            .reserved_cycles_limit
            .as_ref()
            .map(|r| Nat::from(r.get())),
        wasm_memory_limit: args.wasm_memory_limit.as_ref().map(|m| Nat::from(m.get())),
        wasm_memory_threshold: args
            .wasm_memory_threshold
            .as_ref()
            .map(|m| Nat::from(m.get())),
        log_memory_limit: args.log_memory_limit.as_ref().map(|m| Nat::from(m.get())),
        log_visibility: log_visibility.map(Into::into),
        snapshot_visibility: snapshot_visibility.map(Into::into),
        status_visibility: status_visibility.map(Into::into),
        environment_variables,
        // Not exposed as a flag yet; `None` leaves it unchanged.
        minimum_incoming_canister_call_cycles: None,
    };

    proxy_management::update_settings(
        &agent,
        args.proxy,
        UpdateSettingsArgs {
            canister_id: cid,
            settings,
            sender_canister_version: None,
        },
    )
    .await?;

    Ok(())
}

fn compute_allocation_parser(compute_allocation: &str) -> Result<u8, String> {
    if let Ok(num) = compute_allocation.parse::<u8>()
        && num <= 100
    {
        return Ok(num);
    }
    Err("Must be a percent between 0 and 100".to_string())
}

fn visibility_parser(visibility: &str) -> Result<Visibility, String> {
    match visibility {
        "public" => Ok(Visibility::Public),
        "controllers" => Ok(Visibility::Controllers),
        _ => Err("Must be `controllers` or `public`.".to_string()),
    }
}

fn environment_variable_parser(env_var: &str) -> Result<EnvironmentVariable, anyhow::Error> {
    let (name, value) = env_var
        .split_once('=')
        .ok_or(anyhow::anyhow!("invalid environment variable: {}", env_var))?;
    Ok(EnvironmentVariable {
        name: name.to_owned(),
        value: value.to_owned(),
    })
}

fn require_current_settings(args: &UpdateArgs) -> bool {
    if let Some(controllers) = &args.controllers
        && controllers.require_current_settings()
    {
        return true;
    }

    if let Some(log_visibility) = &args.log_visibility
        && log_visibility.require_current_settings()
    {
        return true;
    }

    if let Some(snapshot_visibility) = &args.snapshot_visibility
        && snapshot_visibility.require_current_settings()
    {
        return true;
    }

    if let Some(status_visibility) = &args.status_visibility
        && status_visibility.require_current_settings()
    {
        return true;
    }

    if let Some(environment_variables) = &args.environment_variables
        && environment_variables.require_current_settings()
    {
        return true;
    }

    false
}

fn get_controllers(
    controllers: &ControllerOpt,
    current_status: Option<&CanisterStatusResult>,
) -> Option<Vec<Principal>> {
    if controllers.require_current_settings() {
        let mut current_controllers: HashSet<Principal> = current_status
            .as_ref()
            .expect("current status should be ready")
            .settings
            .controllers
            .clone()
            .into_iter()
            .collect();

        if let Some(to_be_added) = controllers.add_controller.as_ref() {
            current_controllers.extend(to_be_added);
        }
        if let Some(to_be_removed) = controllers.remove_controller.as_ref() {
            for controller in to_be_removed {
                current_controllers.remove(controller);
            }
        }

        Some(current_controllers.into_iter().collect::<Vec<Principal>>())
    } else if controllers.remove_all_controllers {
        Some(controllers.add_controller.clone().unwrap_or_default())
    } else {
        None
    }
}

fn get_environment_variables(
    environment_variables: &EnvironmentVariableOpt,
    current_status: Option<&CanisterStatusResult>,
) -> Option<Vec<EnvironmentVariable>> {
    if environment_variables.require_current_settings() {
        let mut current_environment_variables: HashMap<String, String> = current_status
            .as_ref()
            .expect("current status should be ready")
            .settings
            .environment_variables
            .clone()
            .into_iter()
            .map(|v| (v.name, v.value))
            .collect();

        if let Some(to_be_added) = environment_variables.add_environment_variable.clone() {
            for var in to_be_added {
                current_environment_variables.insert(var.name, var.value);
            }
        }
        if let Some(to_be_removed) = environment_variables.remove_environment_variable.as_ref() {
            for var in to_be_removed {
                current_environment_variables.remove(var);
            }
        }

        return Some(
            current_environment_variables
                .into_iter()
                .map(|(name, value)| EnvironmentVariable { name, value })
                .collect::<Vec<_>>(),
        );
    }

    None
}

fn maybe_warn_on_env_vars_change(
    configured_settings: &icp::canister::Settings,
    environment_variables_opt: &EnvironmentVariableOpt,
) {
    if let Some(configured_vars) = &configured_settings.environment_variables {
        if let Some(to_add) = &environment_variables_opt.add_environment_variable {
            for add_var in to_add {
                if configured_vars.contains_key(&add_var.name) {
                    warn!(
                        "Environment variable '{}' is already set in icp.yaml; this new value will be overridden on next settings sync",
                        add_var.name
                    );
                }
            }
        }
        if let Some(to_remove) = &environment_variables_opt.remove_environment_variable {
            for remove_var in to_remove {
                if configured_vars.contains_key(remove_var) {
                    warn!(
                        "Environment variable '{remove_var}' is already set in icp.yaml; removing it here will be overridden on next settings sync",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    /// The three principals below, in an order no sort would produce, so the
    /// tests can tell a preserved list from a reordered one.
    fn alice() -> Principal {
        principal("ryjl3-tyaaa-aaaaa-aaaba-cai")
    }

    fn bob() -> Principal {
        principal("2vxsx-fae")
    }

    fn carol() -> Principal {
        principal("aaaaa-aa")
    }

    /// Builds a group of flags the way [`LogVisibilityOpt::flags`] does, from
    /// the values clap would have parsed.
    fn flags<'a>(
        fixed: Option<&'a Visibility>,
        add: Option<&'a [Principal]>,
        remove: Option<&'a [Principal]>,
        set: Option<&'a [Principal]>,
    ) -> VisibilityFlags<'a> {
        VisibilityFlags {
            label: "Log visibility",
            stem: "log",
            fixed,
            add,
            remove,
            set,
        }
    }

    /// A fixed policy conflicts with the viewer flags in clap, so this can only
    /// happen if that wiring breaks; resolution still has to pick one, and the
    /// explicit policy is it.
    #[test]
    fn fixed_policy_wins_over_viewer_edits() {
        let viewers = [alice()];
        let resolved = flags(
            Some(&Visibility::Public),
            Some(&viewers),
            None,
            Some(&viewers),
        )
        .resolve(Some(&Visibility::Controllers))
        .unwrap();

        assert_eq!(resolved, Visibility::Public);
    }

    #[test]
    fn set_replaces_the_current_list() {
        let current = Visibility::AllowedViewers(vec![alice(), bob()]);
        let viewers = [carol()];

        assert_eq!(
            flags(None, None, None, Some(&viewers))
                .resolve(Some(&current))
                .unwrap(),
            Visibility::AllowedViewers(vec![carol()])
        );
    }

    #[test]
    fn add_appends_without_duplicating() {
        let current = Visibility::AllowedViewers(vec![alice(), bob()]);
        let to_add = [bob(), carol()];

        assert_eq!(
            flags(None, Some(&to_add), None, None)
                .resolve(Some(&current))
                .unwrap(),
            Visibility::AllowedViewers(vec![alice(), bob(), carol()])
        );
    }

    /// `controllers` carries no list to build on, but a list built from it
    /// grants access on top of the controllers rather than taking any away, so
    /// the edit means what it says and starts from empty.
    #[test]
    fn viewer_edits_against_controllers_start_from_empty() {
        let to_add = [alice()];
        assert_eq!(
            flags(None, Some(&to_add), None, None)
                .resolve(Some(&Visibility::Controllers))
                .unwrap(),
            Visibility::AllowedViewers(vec![alice()])
        );

        let to_remove = [alice()];
        assert_eq!(
            flags(None, None, Some(&to_remove), None)
                .resolve(Some(&Visibility::Controllers))
                .unwrap(),
            Visibility::AllowedViewers(vec![])
        );
    }

    /// `public` carries no list either, and starting one would revoke everyone
    /// else's access — which a relative edit does not say to do, so it is
    /// refused, pointing at the two flags that state a policy outright.
    #[test]
    fn relative_viewer_edits_are_refused_while_public() {
        let viewers = [alice()];

        for (add, remove, named) in [
            (Some(&viewers[..]), None, "--add-log-viewer"),
            (None, Some(&viewers[..]), "--remove-log-viewer"),
            (
                Some(&viewers[..]),
                Some(&viewers[..]),
                "--add-log-viewer / --remove-log-viewer",
            ),
        ] {
            let err = flags(None, add, remove, None)
                .resolve(Some(&Visibility::Public))
                .unwrap_err()
                .to_string();

            assert!(err.contains("Log visibility is currently public"), "{err}");
            assert!(err.contains(named), "{err}");
            assert!(err.contains("--set-log-viewer"), "{err}");
            assert!(err.contains("--log-visibility controllers"), "{err}");
        }
    }

    /// Stating the new list outright is still allowed while public: it says
    /// what the policy becomes, so nothing about it is a surprise.
    #[test]
    fn set_is_allowed_while_public() {
        let viewers = [alice()];

        assert_eq!(
            flags(None, None, None, Some(&viewers))
                .resolve(Some(&Visibility::Public))
                .unwrap(),
            Visibility::AllowedViewers(vec![alice()])
        );
    }

    /// Removal keeps the order of the viewers it leaves behind, since that
    /// order is what `settings show` and `canister status` print.
    #[test]
    fn remove_drops_named_viewers_and_keeps_the_rest_in_order() {
        let current = Visibility::AllowedViewers(vec![alice(), bob(), carol()]);
        let to_remove = [bob()];

        assert_eq!(
            flags(None, None, Some(&to_remove), None)
                .resolve(Some(&current))
                .unwrap(),
            Visibility::AllowedViewers(vec![alice(), carol()])
        );
    }

    /// Removing every viewer leaves the list empty rather than falling back to
    /// `controllers` — the same policy in effect, and what the canister keeps.
    #[test]
    fn remove_can_empty_the_list() {
        let current = Visibility::AllowedViewers(vec![alice()]);
        let to_remove = [alice(), bob()];

        assert_eq!(
            flags(None, None, Some(&to_remove), None)
                .resolve(Some(&current))
                .unwrap(),
            Visibility::AllowedViewers(vec![])
        );
    }

    /// Adding and removing in one command applies both, in that order.
    #[test]
    fn add_and_remove_apply_together() {
        let current = Visibility::AllowedViewers(vec![alice()]);
        let to_add = [bob(), carol()];
        let to_remove = [alice(), bob()];

        assert_eq!(
            flags(None, Some(&to_add), Some(&to_remove), None)
                .resolve(Some(&current))
                .unwrap(),
            Visibility::AllowedViewers(vec![carol()])
        );
    }

    /// The three groups are copies of one another, so the hazard is a
    /// mis-wired `flags()` quietly driving another setting: a group must read
    /// its own flags and name itself.
    #[test]
    fn each_group_reads_its_own_flags() {
        let viewers = vec![alice()];

        let log = LogVisibilityOpt {
            log_visibility: Some(Visibility::Public),
            add_log_viewer: Some(viewers.clone()),
            remove_log_viewer: Some(viewers.clone()),
            set_log_viewer: Some(viewers.clone()),
        };
        let snapshot = SnapshotVisibilityOpt {
            snapshot_visibility: Some(Visibility::Public),
            add_snapshot_viewer: Some(viewers.clone()),
            remove_snapshot_viewer: Some(viewers.clone()),
            set_snapshot_viewer: Some(viewers.clone()),
        };
        let status = StatusVisibilityOpt {
            status_visibility: Some(Visibility::Public),
            add_status_viewer: Some(viewers.clone()),
            remove_status_viewer: Some(viewers.clone()),
            set_status_viewer: Some(viewers.clone()),
        };

        for (flags, label, stem) in [
            (log.flags(), "Log visibility", "log"),
            (snapshot.flags(), "Snapshot visibility", "snapshot"),
            (status.flags(), "Status visibility", "status"),
        ] {
            assert_eq!(flags.label, label);
            assert_eq!(flags.stem, stem);
            assert_eq!(flags.fixed, Some(&Visibility::Public));
            assert_eq!(flags.add, Some(&viewers[..]));
            assert_eq!(flags.remove, Some(&viewers[..]));
            assert_eq!(flags.set, Some(&viewers[..]));
        }

        // And an empty group drives nothing, whichever setting it belongs to.
        for flags in [
            LogVisibilityOpt::default().flags(),
            SnapshotVisibilityOpt::default().flags(),
            StatusVisibilityOpt::default().flags(),
        ] {
            assert_eq!(flags.fixed, None);
            assert!(!flags.require_current_settings());
        }
    }

    /// The error a group raises names the group's own flags, so a refusal
    /// points at the setting the caller was actually editing.
    #[test]
    fn the_public_refusal_names_the_settings_own_flags() {
        let viewers = vec![alice()];

        for (opt_flags, label, stem) in [
            (
                LogVisibilityOpt {
                    add_log_viewer: Some(viewers.clone()),
                    ..<_>::default()
                }
                .flags(),
                "Log visibility",
                "log",
            ),
            (
                SnapshotVisibilityOpt {
                    add_snapshot_viewer: Some(viewers.clone()),
                    ..<_>::default()
                }
                .flags(),
                "Snapshot visibility",
                "snapshot",
            ),
            (
                StatusVisibilityOpt {
                    add_status_viewer: Some(viewers.clone()),
                    ..<_>::default()
                }
                .flags(),
                "Status visibility",
                "status",
            ),
        ] {
            let err = opt_flags
                .resolve(Some(&Visibility::Public))
                .unwrap_err()
                .to_string();

            assert!(
                err.contains(&format!("{label} is currently public")),
                "{err}"
            );
            assert!(err.contains(&format!("--add-{stem}-viewer")), "{err}");
            assert!(err.contains(&format!("--set-{stem}-viewer")), "{err}");
            assert!(
                err.contains(&format!("--{stem}-visibility controllers")),
                "{err}"
            );
        }
    }

    /// Only a fixed policy stands on its own; every viewer edit is resolved
    /// against the current one, so it has to be fetched.
    #[test]
    fn only_a_fixed_policy_needs_no_current_settings() {
        let viewers = [alice()];

        assert!(!flags(Some(&Visibility::Public), None, None, None).require_current_settings());
        assert!(flags(None, Some(&viewers), None, None).require_current_settings());
        assert!(flags(None, None, Some(&viewers), None).require_current_settings());
        assert!(flags(None, None, None, Some(&viewers)).require_current_settings());
    }
}
