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

use crate::{commands::args, operations::proxy_management};

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

    /// Add a principal to the allowed log viewers list
    #[arg(long, action = ArgAction::Append, conflicts_with("set_log_viewer"))]
    add_log_viewer: Option<Vec<Principal>>,

    /// Remove a principal from the allowed log viewers list
    #[arg(long, action = ArgAction::Append, conflicts_with("set_log_viewer"))]
    remove_log_viewer: Option<Vec<Principal>>,

    /// Replace the allowed log viewers list with the specified principals
    #[arg(long, action = ArgAction::Append)]
    set_log_viewer: Option<Vec<Principal>>,
}

impl LogVisibilityOpt {
    fn flags(&self) -> VisibilityFlags<'_> {
        VisibilityFlags {
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

    /// Add a principal to the allowed status viewers list
    #[arg(long, action = ArgAction::Append, conflicts_with("set_status_viewer"))]
    add_status_viewer: Option<Vec<Principal>>,

    /// Remove a principal from the allowed status viewers list
    #[arg(long, action = ArgAction::Append, conflicts_with("set_status_viewer"))]
    remove_status_viewer: Option<Vec<Principal>>,

    /// Replace the allowed status viewers list with the specified principals
    #[arg(long, action = ArgAction::Append)]
    set_status_viewer: Option<Vec<Principal>>,
}

impl StatusVisibilityOpt {
    fn flags(&self) -> VisibilityFlags<'_> {
        VisibilityFlags {
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
    fixed: Option<&'a Visibility>,
    add: Option<&'a [Principal]>,
    remove: Option<&'a [Principal]>,
    set: Option<&'a [Principal]>,
}

impl VisibilityFlags<'_> {
    /// Adding or removing individual viewers is relative to the canister's
    /// current list, so it has to be fetched first.
    fn require_current_settings(&self) -> bool {
        self.add.is_some() || self.remove.is_some()
    }

    fn resolve(&self, current: Option<&Visibility>) -> Visibility {
        if let Some(fixed) = self.fixed {
            return fixed.clone();
        }

        if let Some(viewers) = self.set {
            // TODO(VZ): Warn for switching from public to viewers.
            return Visibility::AllowedViewers(viewers.to_vec());
        }

        let mut viewers = match current {
            Some(Visibility::AllowedViewers(viewers)) => viewers.clone(),
            _ => vec![],
        };

        if let Some(to_be_added) = self.add {
            // TODO(VZ): Warn for switching from public to viewers.
            for principal in to_be_added {
                if !viewers.contains(principal) {
                    viewers.push(*principal);
                }
            }
        }

        if let Some(to_be_removed) = self.remove {
            // TODO(VZ): Warn for removing from if visibility is public and controllers.
            for principal in to_be_removed {
                if let Some(idx) = viewers.iter().position(|x| x == principal) {
                    viewers.swap_remove(idx);
                }
            }
        }

        Visibility::AllowedViewers(viewers)
    }
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

    // Handle log and status visibility.
    let log_visibility = args.log_visibility.as_ref().map(|opt| {
        let current = current_status
            .as_ref()
            .map(|status| Visibility::from(status.settings.log_visibility.clone()));
        opt.flags().resolve(current.as_ref())
    });
    let status_visibility = args.status_visibility.as_ref().map(|opt| {
        let current = current_status
            .as_ref()
            .map(|status| Visibility::from(status.settings.status_visibility.clone()));
        opt.flags().resolve(current.as_ref())
    });

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
        status_visibility: status_visibility.map(Into::into),
        environment_variables,
        // Not exposed as flags yet; `None` leaves them unchanged.
        snapshot_visibility: None,
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

    fn flags<'a>(
        fixed: Option<&'a Visibility>,
        add: Option<&'a [Principal]>,
        remove: Option<&'a [Principal]>,
        set: Option<&'a [Principal]>,
    ) -> VisibilityFlags<'a> {
        VisibilityFlags {
            fixed,
            add,
            remove,
            set,
        }
    }

    #[test]
    fn fixed_policy_wins() {
        let current = Visibility::AllowedViewers(vec![principal("aaaaa-aa")]);
        let resolved = flags(Some(&Visibility::Public), None, None, None).resolve(Some(&current));
        assert_eq!(resolved, Visibility::Public);
    }

    #[test]
    fn set_replaces_the_current_list() {
        let current = Visibility::AllowedViewers(vec![principal("aaaaa-aa")]);
        let set = [principal("2vxsx-fae")];
        let resolved = flags(None, None, None, Some(&set)).resolve(Some(&current));
        assert_eq!(resolved, Visibility::AllowedViewers(set.to_vec()));
    }

    #[test]
    fn add_and_remove_apply_to_the_current_list() {
        let current =
            Visibility::AllowedViewers(vec![principal("aaaaa-aa"), principal("2vxsx-fae")]);
        let add = [principal("ryjl3-tyaaa-aaaaa-aaaba-cai")];
        let remove = [principal("aaaaa-aa")];

        let resolved = flags(None, Some(&add), Some(&remove), None).resolve(Some(&current));
        let Visibility::AllowedViewers(viewers) = resolved else {
            panic!("expected allowed viewers");
        };
        assert_eq!(viewers.len(), 2);
        assert!(viewers.contains(&principal("2vxsx-fae")));
        assert!(viewers.contains(&principal("ryjl3-tyaaa-aaaaa-aaaba-cai")));
    }

    #[test]
    fn adding_an_existing_viewer_does_not_duplicate_it() {
        let current = Visibility::AllowedViewers(vec![principal("aaaaa-aa")]);
        let add = [principal("aaaaa-aa")];
        let resolved = flags(None, Some(&add), None, None).resolve(Some(&current));
        assert_eq!(
            resolved,
            Visibility::AllowedViewers(vec![principal("aaaaa-aa")])
        );
    }

    #[test]
    fn adding_a_viewer_to_a_non_list_policy_starts_from_empty() {
        let add = [principal("aaaaa-aa")];
        let resolved = flags(None, Some(&add), None, None).resolve(Some(&Visibility::Public));
        assert_eq!(
            resolved,
            Visibility::AllowedViewers(vec![principal("aaaaa-aa")])
        );
    }

    #[test]
    fn only_viewer_edits_need_the_current_settings() {
        let viewers = [principal("aaaaa-aa")];
        assert!(flags(None, Some(&viewers), None, None).require_current_settings());
        assert!(flags(None, None, Some(&viewers), None).require_current_settings());
        assert!(!flags(None, None, None, Some(&viewers)).require_current_settings());
        assert!(!flags(Some(&Visibility::Public), None, None, None).require_current_settings());
    }

    #[test]
    fn parser_accepts_only_the_fixed_policies() {
        assert_eq!(
            visibility_parser("controllers"),
            Ok(Visibility::Controllers)
        );
        assert_eq!(visibility_parser("public"), Ok(Visibility::Public));
        assert!(visibility_parser("allowed_viewers").is_err());
    }
}
