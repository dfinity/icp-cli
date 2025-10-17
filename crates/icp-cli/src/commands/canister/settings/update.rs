use std::collections::{HashMap, HashSet};

use byte_unit::{Byte, Unit};
use clap::{ArgAction, Args};
use ic_agent::{AgentError, export::Principal};
use ic_management_canister_types::{CanisterStatusResult, EnvironmentVariable, LogVisibility};
use icp::{agent, identity, network};

use crate::{
    commands::{Context, Mode},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Clone, Debug, Default, Args)]
pub(crate) struct ControllerOpt {
    #[arg(long, action = ArgAction::Append, conflicts_with("set_controller"))]
    add_controller: Option<Vec<Principal>>,

    #[arg(long, action = ArgAction::Append, conflicts_with("set_controller"))]
    remove_controller: Option<Vec<Principal>>,

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
    /// The name of the canister within the current project
    pub(crate) name: String,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    #[command(flatten)]
    pub(crate) controllers: Option<ControllerOpt>,

    #[arg(long, value_parser = compute_allocation_parser)]
    pub(crate) compute_allocation: Option<u8>,

    #[arg(long, value_parser = memory_parser)]
    pub(crate) memory_allocation: Option<Byte>,

    #[arg(long, value_parser = freezing_threshold_parser)]
    pub(crate) freezing_threshold: Option<u64>,

    #[arg(long, value_parser = reserved_cycles_limit_parser)]
    pub(crate) reserved_cycles_limit: Option<u128>,

    #[arg(long, value_parser = memory_parser)]
    pub(crate) wasm_memory_limit: Option<Byte>,

    #[arg(long, value_parser = memory_parser)]
    pub(crate) wasm_memory_threshold: Option<Byte>,

    #[command(flatten)]
    pub(crate) log_visibility: Option<LogVisibilityOpt>,

    #[command(flatten)]
    pub(crate) environment_variables: Option<EnvironmentVariableOpt>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error("invalid environment variable '{variable}'")]
    InvalidEnvironmentVariable { variable: String },

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Update(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &UpdateArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            // Load project
            let p = ctx.project.load().await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            // Ensure canister is included in the environment
            if !env.canisters.contains_key(&args.name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: args.name.to_owned(),
                });
            }

            // Lookup the canister id
            let cid = ctx.ids.lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: args.name.to_owned(),
            })?;

            // Management Interface
            let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

            let mut current_status: Option<CanisterStatusResult> = None;
            if require_current_settings(args) {
                current_status = Some(mgmt.canister_status(&cid).await?.0);
            }

            // TODO(VZ): Ask for consent
            // - if the freezing threshold is too long or too short.
            // - if trying to remove the caller itself from the controllers.

            // Handle controllers.
            let mut controllers: Option<Vec<Principal>> = None;
            if let Some(controllers_opt) = &args.controllers {
                controllers = get_controllers(controllers_opt, current_status.as_ref());
            }

            // Handle log visibility.
            let mut log_visibility: Option<LogVisibility> = None;
            if let Some(log_visibility_opt) = args.log_visibility.clone() {
                log_visibility = get_log_visibility(&log_visibility_opt, current_status.as_ref());
            }

            // Handle environment variables.
            let mut environment_variables: Option<Vec<EnvironmentVariable>> = None;
            if let Some(environment_variables_opt) = &args.environment_variables {
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
                update = update.with_compute_allocation(compute_allocation);
            }
            if let Some(memory_allocation) = args.memory_allocation {
                update = update.with_memory_allocation(memory_allocation.as_u64());
            }
            if let Some(freezing_threshold) = args.freezing_threshold {
                update = update.with_freezing_threshold(freezing_threshold);
            }
            if let Some(reserved_cycles_limit) = args.reserved_cycles_limit {
                update = update.with_reserved_cycles_limit(reserved_cycles_limit);
            }
            if let Some(wasm_memory_limit) = args.wasm_memory_limit {
                update = update.with_wasm_memory_limit(wasm_memory_limit.as_u64());
            }
            if let Some(wasm_memory_threshold) = args.wasm_memory_threshold {
                update = update.with_wasm_memory_threshold(wasm_memory_threshold.as_u64());
            }
            if let Some(log_visibility) = log_visibility {
                update = update.with_log_visibility(log_visibility);
            }
            if let Some(environment_variables) = environment_variables {
                update = update.with_environment_variables(environment_variables);
            }
            update.await?;
        }
    }

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

fn reserved_cycles_limit_parser(reserved_cycles_limit: &str) -> Result<u128, String> {
    if let Ok(num) = reserved_cycles_limit.parse::<u128>() {
        return Ok(num);
    }
    Err("Must be a value between 0..2^128-1 inclusive".to_string())
}

fn log_visibility_parser(log_visibility: &str) -> Result<LogVisibility, String> {
    match log_visibility {
        "public" => Ok(LogVisibility::Public),
        "controllers" => Ok(LogVisibility::Controllers),
        _ => Err("Must be `controllers` or `public`.".to_string()),
    }
}

fn environment_variable_parser(env_var: &str) -> Result<EnvironmentVariable, CommandError> {
    let (name, value) =
        env_var
            .split_once('=')
            .ok_or(CommandError::InvalidEnvironmentVariable {
                variable: env_var.to_owned(),
            })?;
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
