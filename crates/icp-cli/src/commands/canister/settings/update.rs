use anyhow::bail;
use byte_unit::{Byte, Unit};
use clap::{ArgAction, Args};
use console::Term;
use dialoguer::Confirm;
use ic_agent::Identity;
use ic_agent::export::Principal;
use ic_management_canister_types::{CanisterStatusResult, EnvironmentVariable, LogVisibility};
use icp::ProjectLoadError;
use icp::context::{CanisterSelection, Context};
use std::collections::{HashMap, HashSet};
use std::io::Write;

use crate::commands::args;
use crate::commands::parsers::parse_cycles_amount;

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct ControllerOpt {
    /// Add one or more principals to the canister's controller list.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_controller"))]
    add_controller: Option<Vec<Principal>>,

    /// Remove one or more principals from the canister's controller list.
    ///
    /// Warning: Removing yourself will cause you to lose control of the canister.
    #[arg(long, action = ArgAction::Append, conflicts_with("set_controller"))]
    remove_controller: Option<Vec<Principal>>,

    /// Replace the canister's controller list with the specified principals.
    ///
    /// Warning: This removes all existing controllers not in the new list.
    /// If you don't include yourself, you will lose control of the canister.
    #[arg(long, action = ArgAction::Append)]
    set_controller: Option<Vec<Principal>>,
}

impl ControllerOpt {
    pub(crate) fn require_current_settings(&self) -> bool {
        self.add_controller.is_some() || self.remove_controller.is_some()
    }
}

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct LogVisibilityOpt {
    #[arg(
        long,
        value_parser = log_visibility_parser,
        conflicts_with("add_log_viewer"),
        conflicts_with("remove_log_viewer"),
        conflicts_with("set_log_viewer"),
    )]
    log_visibility: Option<LogVisibility>,

    #[arg(long, action = ArgAction::Append, conflicts_with("set_log_viewer"))]
    add_log_viewer: Option<Vec<Principal>>,

    #[arg(long, action = ArgAction::Append, conflicts_with("set_log_viewer"))]
    remove_log_viewer: Option<Vec<Principal>>,

    #[arg(long, action = ArgAction::Append)]
    set_log_viewer: Option<Vec<Principal>>,
}

impl LogVisibilityOpt {
    pub(crate) fn require_current_settings(&self) -> bool {
        self.add_log_viewer.is_some() || self.remove_log_viewer.is_some()
    }
}

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct EnvironmentVariableOpt {
    #[arg(long, value_parser = environment_variable_parser, action = ArgAction::Append)]
    add_environment_variable: Option<Vec<EnvironmentVariable>>,

    #[arg(long, action = ArgAction::Append)]
    remove_environment_variable: Option<Vec<String>>,
}

impl EnvironmentVariableOpt {
    pub(crate) fn require_current_settings(&self) -> bool {
        self.add_environment_variable.is_some() || self.remove_environment_variable.is_some()
    }
}

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Force the operation without confirmation prompts
    #[arg(short = 'f', long)]
    force: bool,

    #[command(flatten)]
    controllers: Option<ControllerOpt>,

    #[arg(long, value_parser = compute_allocation_parser)]
    compute_allocation: Option<u8>,

    #[arg(long, value_parser = memory_parser)]
    memory_allocation: Option<Byte>,

    #[arg(long, value_parser = freezing_threshold_parser)]
    freezing_threshold: Option<u64>,

    /// Reserved cycles limit for the canister.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, value_parser = parse_cycles_amount)]
    reserved_cycles_limit: Option<u128>,

    #[arg(long, value_parser = memory_parser)]
    wasm_memory_limit: Option<Byte>,

    #[arg(long, value_parser = memory_parser)]
    wasm_memory_threshold: Option<Byte>,

    #[command(flatten)]
    log_visibility: Option<LogVisibilityOpt>,

    #[command(flatten)]
    environment_variables: Option<EnvironmentVariableOpt>,
}

pub(crate) async fn exec(ctx: &Context, args: &UpdateArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let identity = ctx.get_identity(&selections.identity).await?;
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

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    let mut current_status: Option<CanisterStatusResult> = None;
    if require_current_settings(args) {
        current_status = Some(mgmt.canister_status(&cid).await?.0);
    }

    // TODO(VZ): Ask for consent if the freezing threshold is too long or too short.

    // Handle controllers.
    let mut controllers: Option<Vec<Principal>> = None;
    if let Some(controllers_opt) = &args.controllers {
        controllers = get_controllers(controllers_opt, current_status.as_ref());

        // Check if the caller is being removed from controllers
        if let Some(new_controllers) = &controllers
            && !new_controllers.contains(&caller_principal)
            && !args.force
        {
            ctx.term.write_line(
                "Warning: You are about to remove yourself from the controllers list.",
            )?;
            ctx.term.write_line(
                "This will cause you to lose control of the canister and cannot be undone.",
            )?;

            let confirmed = Confirm::new()
                .with_prompt("Do you want to proceed?")
                .default(false)
                .interact()?;

            if !confirmed {
                bail!("Operation cancelled by user");
            }
        }
    }

    // Handle log visibility.
    let mut log_visibility: Option<LogVisibility> = None;
    if let Some(log_visibility_opt) = args.log_visibility.clone() {
        log_visibility = get_log_visibility(&log_visibility_opt, current_status.as_ref());
    }

    // Handle environment variables.
    let mut environment_variables: Option<Vec<EnvironmentVariable>> = None;
    if let Some(environment_variables_opt) = &args.environment_variables {
        maybe_warn_on_env_vars_change(&ctx.term, &configured_settings, environment_variables_opt)?;
        environment_variables =
            get_environment_variables(environment_variables_opt, current_status.as_ref());
    }

    // Update settings.
    let mut update = mgmt.update_settings(&cid);
    if let Some(controllers) = controllers {
        for controller in controllers {
            update = update.with_controller(controller);
        }
    }
    if let Some(compute_allocation) = args.compute_allocation {
        if configured_settings.compute_allocation.is_some() {
            ctx.term.write_line(
                "Warning: Compute allocation is already set in icp.yaml; this new value will be overridden on next settings sync"
            )?
        }
        update = update.with_compute_allocation(compute_allocation);
    }
    if let Some(memory_allocation) = args.memory_allocation {
        if configured_settings.memory_allocation.is_some() {
            ctx.term.write_line(
                "Warning: Memory allocation is already set in icp.yaml; this new value will be overridden on next settings sync"
            )?
        }
        update = update.with_memory_allocation(memory_allocation.as_u64());
    }
    if let Some(freezing_threshold) = args.freezing_threshold {
        if configured_settings.freezing_threshold.is_some() {
            ctx.term.write_line(
                "Warning: Freezing threshold is already set in icp.yaml; this new value will be overridden on next settings sync"
            )?
        }
        update = update.with_freezing_threshold(freezing_threshold);
    }
    if let Some(reserved_cycles_limit) = args.reserved_cycles_limit {
        if configured_settings.reserved_cycles_limit.is_some() {
            ctx.term.write_line(
                "Warning: Reserved cycles limit is already set in icp.yaml; this new value will be overridden on next settings sync"
            )?
        }
        update = update.with_reserved_cycles_limit(reserved_cycles_limit);
    }
    if let Some(wasm_memory_limit) = args.wasm_memory_limit {
        if configured_settings.wasm_memory_limit.is_some() {
            ctx.term.write_line(
                "Warning: Wasm memory limit is already set in icp.yaml; this new value will be overridden on next settings sync"
            )?
        }
        update = update.with_wasm_memory_limit(wasm_memory_limit.as_u64());
    }
    if let Some(wasm_memory_threshold) = args.wasm_memory_threshold {
        if configured_settings.wasm_memory_threshold.is_some() {
            ctx.term.write_line(
                "Warning: Wasm memory threshold is already set in icp.yaml; this new value will be overridden on next settings sync"
            )?
        }
        update = update.with_wasm_memory_threshold(wasm_memory_threshold.as_u64());
    }
    if let Some(log_visibility) = log_visibility {
        update = update.with_log_visibility(log_visibility);
    }
    if let Some(environment_variables) = environment_variables {
        update = update.with_environment_variables(environment_variables);
    }
    update.await?;

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

fn memory_parser(memory_allocation: &str) -> Result<Byte, String> {
    let limit = Byte::from_u64_with_unit(256, Unit::TiB).expect("256 TiB is a valid byte unit");
    if let Ok(byte) = memory_allocation.parse::<Byte>()
        && byte <= limit
    {
        return Ok(byte);
    }
    Err("Must be a value between 0..256 TiB inclusive, (e.g. '2GiB')".to_string())
}

fn freezing_threshold_parser(freezing_threshold: &str) -> Result<u64, String> {
    if let Ok(num) = freezing_threshold.parse::<u64>() {
        return Ok(num);
    }
    Err("Must be a value between 0..2^64-1 inclusive".to_string())
}

fn log_visibility_parser(log_visibility: &str) -> Result<LogVisibility, String> {
    match log_visibility {
        "public" => Ok(LogVisibility::Public),
        "controllers" => Ok(LogVisibility::Controllers),
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
    if let Some(controllers) = controllers.set_controller.as_ref() {
        return Some(controllers.clone());
    } else if controllers.require_current_settings() {
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

        return Some(current_controllers.into_iter().collect::<Vec<Principal>>());
    }

    None
}

fn get_log_visibility(
    log_visibility: &LogVisibilityOpt,
    current_status: Option<&CanisterStatusResult>,
) -> Option<LogVisibility> {
    if let Some(log_visibility) = log_visibility.log_visibility.as_ref() {
        return Some(log_visibility.clone());
    }

    if let Some(viewer) = log_visibility.set_log_viewer.as_ref() {
        // TODO(VZ): Warn for switching from public to viewers.
        return Some(LogVisibility::AllowedViewers(viewer.clone()));
    }

    let mut log_viewers: Vec<Principal> = match current_status {
        Some(status) => match &status.settings.log_visibility {
            LogVisibility::AllowedViewers(viewers) => viewers.clone(),
            _ => vec![],
        },
        None => vec![],
    };

    if let Some(to_be_added) = log_visibility.add_log_viewer.as_ref() {
        // TODO(VZ): Warn for switching from public to viewers.
        for principal in to_be_added {
            if !log_viewers.iter().any(|x| x == principal) {
                log_viewers.push(*principal);
            }
        }
    }

    if let Some(removed) = log_visibility.remove_log_viewer.as_ref() {
        // TODO(VZ): Warn for removing from if log visibility is public and controllers.
        for principal in removed {
            if let Some(idx) = log_viewers.iter().position(|x| x == principal) {
                log_viewers.swap_remove(idx);
            }
        }
    }

    Some(LogVisibility::AllowedViewers(log_viewers))
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
    mut term: &Term,
    configured_settings: &icp::canister::Settings,
    environment_variables_opt: &EnvironmentVariableOpt,
) -> Result<(), anyhow::Error> {
    if let Some(configured_vars) = &configured_settings.environment_variables {
        if let Some(to_add) = &environment_variables_opt.add_environment_variable {
            for add_var in to_add {
                if configured_vars.contains_key(&add_var.name) {
                    writeln!(
                        term,
                        "Warning: Environment variable '{}' is already set in icp.yaml; this new value will be overridden on next settings sync",
                        add_var.name
                    )?;
                }
            }
        }
        if let Some(to_remove) = &environment_variables_opt.remove_environment_variable {
            for remove_var in to_remove {
                if configured_vars.contains_key(remove_var) {
                    writeln!(
                        term,
                        "Warning: Environment variable '{}' is already set in icp.yaml; removing it here will be overridden on next settings sync",
                        remove_var
                    )?;
                }
            }
        }
    }
    Ok(())
}
