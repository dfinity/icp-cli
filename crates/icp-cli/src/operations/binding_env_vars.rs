use std::collections::{BTreeMap, HashSet};

use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError, export::Principal};
use ic_utils::interfaces::{
    ManagementCanister, management_canister::builders::EnvironmentVariable,
};
use icp::Canister;
use snafu::Snafu;

use crate::progress::{ProgressManager, ProgressManagerSettings};

#[derive(Debug, Snafu)]
pub enum BindingEnvVarsOperationError {
    #[snafu(display("Could not find canister id(s) for {} in environment '{environment}'. Make sure they are created first", canister_names.join(", ")))]
    CanisterNotCreated {
        environment: String,
        canister_names: Vec<String>,
    },

    #[snafu(display("agent error: {source}"))]
    Agent { source: AgentError },
}

pub(crate) async fn set_env_vars_for_canister(
    mgmt: &ManagementCanister<'_>,
    canister_id: &Principal,
    canister_info: &Canister,
    binding_vars: &[(String, String)],
) -> Result<(), BindingEnvVarsOperationError> {
    let mut environment_variables = canister_info
        .settings
        .environment_variables
        .to_owned()
        .unwrap_or_default();

    // inject the ids of the other canisters
    for (k, v) in binding_vars.iter() {
        environment_variables.insert(k.to_string(), v.to_string());
    }

    let environment_variables = environment_variables
        .into_iter()
        .map(|(name, value)| EnvironmentVariable { name, value })
        .collect::<Vec<_>>();
    mgmt.update_settings(canister_id)
        .with_environment_variables(environment_variables)
        .await
        .map_err(|source| BindingEnvVarsOperationError::Agent { source })?;

    Ok(())
}

/// Orchestrates setting environment variables for multiple canisters with progress tracking
pub(crate) async fn set_binding_env_vars_many(
    agent: Agent,
    environment_name: &str,
    target_canisters: Vec<(Principal, Canister)>,
    canister_list: BTreeMap<String, Principal>,
    debug: bool,
) -> Result<(), BindingEnvVarsOperationError> {
    let mgmt = ManagementCanister::create(&agent);

    // Check that all the canisters in this environment have an id
    // We need to have all the ids to generate environment variables
    // for the bindings
    let canisters_with_ids: HashSet<&String> = canister_list.keys().collect();

    let all_canister_names: Vec<String> = target_canisters
        .iter()
        .map(|(_, info)| info.name.clone())
        .collect();

    let missing_canisters: Vec<String> = all_canister_names
        .iter()
        .filter(|c| !canisters_with_ids.contains(*c))
        .map(|c| c.to_string())
        .collect();

    if !missing_canisters.is_empty() {
        return Err(BindingEnvVarsOperationError::CanisterNotCreated {
            environment: environment_name.to_owned(),
            canister_names: missing_canisters,
        });
    }

    let binding_vars = canister_list
        .iter()
        .map(|(n, p)| (format!("PUBLIC_CANISTER_ID:{n}"), p.to_text()))
        .collect::<Vec<(_, _)>>();

    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (cid, info) in target_canisters {
        let pb = progress_manager.create_progress_bar(&info.name);

        let settings_fn = {
            let mgmt = mgmt.clone();
            let pb = pb.clone();
            let binding_vars = binding_vars.clone();

            async move {
                pb.set_message("Updating environment variables...");
                set_env_vars_for_canister(&mgmt, &cid, &info, &binding_vars).await
            }
        };

        futs.push_back(async move {
            ProgressManager::execute_with_progress(
                &pb,
                settings_fn,
                || "Environment variables updated successfully".to_string(),
                |err| format!("Failed to update environment variables: {err}"),
            )
            .await
        });
    }

    while let Some(res) = futs.next().await {
        res?;
    }

    Ok(())
}
