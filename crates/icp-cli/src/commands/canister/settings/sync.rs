use std::collections::HashMap;

use crate::options::{EnvironmentOpt, IdentityOpt, NetworkOpt};

use clap::Args;
use ic_agent::AgentError;
use ic_management_canister_types::EnvironmentVariable;
use ic_utils::interfaces::ManagementCanister;
use icp::{
    LoadError,
    canister::Settings,
    context::{
        Context, EnvironmentSelection, GetAgentForEnvError, GetCanisterIdForEnvError,
        GetEnvironmentError,
    },
};
use itertools::Itertools;
use snafu::{ResultExt, Snafu};

#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    name: String,
    #[command(flatten)]
    pub(crate) network: NetworkOpt,
    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

#[derive(Debug, Snafu)]
pub(crate) enum CommandError {
    #[snafu(transparent)]
    GetAgentForEnv { source: GetAgentForEnvError },
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },
    #[snafu(transparent)]
    GetCanisterIdForEnv { source: GetCanisterIdForEnvError },
    #[snafu(transparent)]
    LoadProject { source: LoadError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{name}'"))]
    EnvironmentCanisterNotFound { name: String, environment: String },
    #[snafu(display("failed to fetch current canister settings"))]
    FetchCurrentSettings { source: AgentError },
    #[snafu(display("invalid canister settings in manifest"))]
    ValidateSettings { source: AgentError },
    #[snafu(display("failed to update canister settings"))]
    UpdateSettings { source: AgentError },
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), CommandError> {
    let environment_selection: EnvironmentSelection = args.environment.clone().into();
    let name = &args.name;

    let p = ctx.project.load().await?;
    let env = ctx.get_environment(&environment_selection).await?;

    let Some((_, canister)) = p.canisters.get(name) else {
        return CanisterNotFoundSnafu { name }.fail();
    };

    if !env.canisters.contains_key(&args.name) {
        return EnvironmentCanisterNotFoundSnafu {
            environment: &env.name,
            name,
        }
        .fail();
    }

    let agent = ctx
        .get_agent_for_env(&args.identity.clone().into(), &environment_selection)
        .await?;
    let cid = ctx
        .get_canister_id_for_env(&args.name, &environment_selection)
        .await?;
    let mgmt = ManagementCanister::create(&agent);

    let (status,) = mgmt
        .canister_status(&cid)
        .await
        .context(FetchCurrentSettingsSnafu)?;
    let &Settings {
        compute_allocation,
        memory_allocation,
        freezing_threshold,
        reserved_cycles_limit,
        wasm_memory_limit,
        wasm_memory_threshold,
        ref environment_variables,
    } = &canister.settings;
    let current_settings = status.settings;
    let environment_variable_setting =
        if let Some(configured_environment_variables) = &environment_variables {
            let mut merged_environment_variables: HashMap<_, _> = current_settings
                .environment_variables
                .into_iter()
                .map(|EnvironmentVariable { name, value }| (name, value))
                .collect();
            merged_environment_variables.extend(configured_environment_variables.clone());
            Some(
                merged_environment_variables
                    .into_iter()
                    .map(|(name, value)| EnvironmentVariable { name, value })
                    .collect_vec(),
            )
        } else {
            None
        };

    mgmt.update_settings(&cid)
        .with_optional_compute_allocation(compute_allocation)
        .with_optional_memory_allocation(memory_allocation)
        .with_optional_freezing_threshold(freezing_threshold)
        .with_optional_reserved_cycles_limit(reserved_cycles_limit)
        .with_optional_wasm_memory_limit(wasm_memory_limit)
        .with_optional_wasm_memory_threshold(wasm_memory_threshold)
        .with_optional_environment_variables(environment_variable_setting)
        .build()
        .context(ValidateSettingsSnafu)?
        .await
        .context(UpdateSettingsSnafu)?;

    Ok(())
}
