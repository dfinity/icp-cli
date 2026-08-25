use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use icp_deploy_canister::apply_binding_env_vars;
use icp_events::TaskOutcome;
use snafu::Snafu;
use tracing::error;

use crate::Canister;
use crate::operations::access::AgentIcpAccess;
use crate::operations::task::{Reporter, Task};

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to update environment variables."))]
pub struct SetBindingEnvVarsManyError {
    names: Vec<String>,
}

/// Orchestrates setting environment variables for multiple canisters concurrently.
///
/// The per-canister work (computing the generated `PUBLIC_CANISTER_ID:*`
/// bindings, merging with manifest env vars, and applying them) lives in
/// `icp_deploy_canister::apply_binding_env_vars`; this wrapper only adds the
/// missing-id precheck and the progress reporting.
pub async fn set_binding_env_vars_many(
    agent: Agent,
    proxy: Option<Principal>,
    environment_name: &str,
    target_canisters: Vec<(Principal, Canister)>,
    canister_list: BTreeMap<String, Principal>,
    reporter: &Reporter,
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

    for (cid, info) in target_canisters {
        let task = reporter.task(Task::update_environment_variables(info.name.clone(), cid));
        let icp = icp.clone();
        let canister_list = canister_list.clone();

        futs.push_back(async move {
            let result = apply_binding_env_vars(&info, cid, &canister_list, icp.as_ref()).await;

            match &result {
                Ok(()) => task.finish(TaskOutcome::succeeded()),
                Err(error) => task.finish(TaskOutcome::failed(error.to_string())),
            }

            result.map_err(|_| info.name.clone())
        });
    }

    // Collect the failed canister names; the renderer owns displaying each
    // failure.
    let mut failed: Vec<String> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(name) = res {
            failed.push(name);
        }
    }

    if !failed.is_empty() {
        return SetBindingEnvVarsManySnafu { names: failed }.fail();
    }

    Ok(())
}
