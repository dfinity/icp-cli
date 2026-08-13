use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use icp::Canister;
use icp_deploy_canister::{SyncCanisterError, apply_binding_env_vars};
use snafu::Snafu;
use tracing::error;

use crate::operations::access::AgentIcpAccess;
use crate::progress::{ProgressManager, ProgressManagerSettings};

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to update environment variables."))]
pub struct SetBindingEnvVarsManyError {
    names: Vec<String>,
}

/// Holds error information from a failed environment variable update operation
struct BindingEnvVarsFailure {
    canister_name: String,
    canister_id: Principal,
    error: SyncCanisterError,
}

/// Orchestrates setting environment variables for multiple canisters with progress tracking.
///
/// The per-canister work (computing the generated `PUBLIC_CANISTER_ID:*`
/// bindings, merging with manifest env vars, and applying them) lives in
/// `icp_deploy_canister::apply_binding_env_vars`; this wrapper only adds the
/// missing-id precheck and progress display.
pub(crate) async fn set_binding_env_vars_many(
    agent: Agent,
    proxy: Option<Principal>,
    environment_name: &str,
    target_canisters: Vec<(Principal, Canister)>,
    canister_list: BTreeMap<String, Principal>,
    debug: bool,
) -> Result<(), SetBindingEnvVarsManyError> {
    // Check that all the canisters in this environment have an id: we need all
    // ids to generate the binding environment variables.
    let canisters_with_ids: HashSet<&String> = canister_list.keys().collect();

    let missing_canisters: Vec<String> = target_canisters
        .iter()
        .map(|(_, info)| info.name.clone())
        .filter(|c| !canisters_with_ids.contains(c))
        .collect();

    if !missing_canisters.is_empty() {
        error!(
            "----- Error: Could not find canister id(s) for {} in environment '{}' -----",
            missing_canisters.join(", "),
            environment_name
        );
        error!("Make sure they are created first");

        return SetBindingEnvVarsManySnafu {
            names: missing_canisters,
        }
        .fail();
    }

    let icp = Arc::new(AgentIcpAccess::new(agent, proxy));
    let canister_list = Arc::new(canister_list);

    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (cid, info) in target_canisters {
        let pb = progress_manager.create_progress_bar(&info.name);
        let canister_name = info.name.clone();
        let icp = icp.clone();
        let canister_list = canister_list.clone();

        let settings_fn = {
            let pb = pb.clone();
            async move {
                pb.set_message("Updating environment variables...");
                apply_binding_env_vars(&info, cid, &canister_list, icp.as_ref()).await
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
            error!(
                "----- Failed to update environment variables for canister '{}': {} -----",
                failure.canister_name, failure.canister_id,
            );
            error!("'{}'", failure.error);
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
