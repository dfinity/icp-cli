use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError, export::Principal};
use ic_utils::interfaces::{
    ManagementCanister, management_canister::builders::EnvironmentVariable,
};
use icp::{Canister, context::TermWriter};
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

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to update environment variables."))]
pub struct SetBindingEnvVarsManyError {
    names: Vec<String>,
}

/// Holds error information from a failed environment variable update operation
struct BindingEnvVarsFailure {
    canister_name: String,
    canister_id: Principal,
    error: BindingEnvVarsOperationError,
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
    term: Arc<TermWriter>,
    debug: bool,
) -> Result<(), SetBindingEnvVarsManyError> {
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
        let _ = term.write_line("");
        let _ = term.write_line("");
        let _ = term.write_line(&format!(
            " ----- Error: Could not find canister id(s) for {} in environment '{}' -----",
            missing_canisters.join(", "),
            environment_name
        ));
        let _ = term.write_line("Make sure they are created first");
        let _ = term.write_line("");

        return SetBindingEnvVarsManySnafu {
            names: missing_canisters,
        }
        .fail();
    }

    let binding_vars = canister_list
        .iter()
        .map(|(n, p)| (format!("PUBLIC_CANISTER_ID:{n}"), p.to_text()))
        .collect::<Vec<(_, _)>>();

    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (cid, info) in target_canisters {
        let pb = progress_manager.create_progress_bar(&info.name);
        let canister_name = info.name.clone();

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
            let result = ProgressManager::execute_with_progress(
                &pb,
                settings_fn,
                || "Environment variables updated successfully".to_string(),
                |err| format!("Failed to update environment variables: {err}"),
            )
            .await;

            // Map error to include canister context for deferred printing
            result.map_err(|error| BindingEnvVarsFailure {
                canister_name,
                canister_id: cid,
                error,
            })
        });
    }

    // Consume the set of futures and collect errors
    let mut errors: Vec<BindingEnvVarsFailure> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(failure) = res {
            errors.push(failure);
        }
    }

    if !errors.is_empty() {
        // Print all errors in batch
        for failure in &errors {
            let _ = term.write_line("");
            let _ = term.write_line("");
            let _ = term.write_line(&format!(
                " ----- Failed to update environment variables for canister '{}': {} -----",
                failure.canister_name, failure.canister_id,
            ));
            let _ = term.write_line(&format!("Error: '{}'", failure.error));
            let _ = term.write_line("");
        }

        return SetBindingEnvVarsManySnafu {
            names: errors
                .iter()
                .map(|e| e.canister_name.clone())
                .collect::<Vec<String>>(),
        }
        .fail();
    }

    Ok(())
}
