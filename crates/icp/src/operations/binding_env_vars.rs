use std::collections::{BTreeMap, HashSet};

use crate::Canister;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use ic_management_canister_types::{CanisterSettings, EnvironmentVariable, UpdateSettingsArgs};
use icp_events::{Reporter, TaskKind, TaskOutcome};
use snafu::Snafu;
use tracing::error;

use super::proxy::UpdateOrProxyError;
use super::proxy_management;

#[derive(Debug, Snafu)]
pub enum BindingEnvVarsOperationError {
    #[snafu(display("Could not find canister id(s) for {} in environment '{environment}'. Make sure they are created first", canister_names.join(", ")))]
    CanisterNotCreated {
        environment: String,
        canister_names: Vec<String>,
    },

    #[snafu(transparent)]
    UpdateOrProxy { source: UpdateOrProxyError },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to update environment variables."))]
pub struct SetBindingEnvVarsManyError {
    names: Vec<String>,
}

pub async fn set_env_vars_for_canister(
    agent: &Agent,
    proxy: Option<Principal>,
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

    proxy_management::update_settings(
        agent,
        proxy,
        UpdateSettingsArgs {
            canister_id: *canister_id,
            settings: CanisterSettings {
                environment_variables: Some(environment_variables),
                ..Default::default()
            },
            sender_canister_version: None,
        },
    )
    .await?;

    Ok(())
}

/// Orchestrates setting environment variables for multiple canisters concurrently.
pub async fn set_binding_env_vars_many(
    agent: Agent,
    proxy: Option<Principal>,
    environment_name: &str,
    target_canisters: Vec<(Principal, Canister)>,
    canister_list: BTreeMap<String, Principal>,
    reporter: &Reporter,
) -> Result<(), SetBindingEnvVarsManyError> {
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

    let mut futs = FuturesOrdered::new();

    for (cid, info) in target_canisters {
        let task = reporter.task(TaskKind::UpdateEnvironmentVariables {
            canister: info.name.clone(),
            canister_id: cid,
        });

        // Each canister receives only the ids it is wired to (its own project's
        // canisters by their local names, plus any declared dependencies under
        // their aliases), resolved to the ids that exist in this environment.
        // A project without dependencies wires every canister to every sibling,
        // reproducing the previous flat behavior.
        let binding_vars: Vec<(String, String)> = info
            .bindings
            .iter()
            .filter_map(|(env_name, referenced_key)| {
                canister_list.get(referenced_key).map(|principal| {
                    (
                        format!("PUBLIC_CANISTER_ID:{env_name}"),
                        principal.to_text(),
                    )
                })
            })
            .collect();

        let agent = agent.clone();
        futs.push_back(async move {
            let result = set_env_vars_for_canister(&agent, proxy, &cid, &info, &binding_vars).await;

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
