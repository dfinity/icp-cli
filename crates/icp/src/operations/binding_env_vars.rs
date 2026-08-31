use std::collections::{BTreeMap, HashSet};

use crate::Canister;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use ic_management_canister_types::{
    CanisterIdRecord, CanisterSettings, EnvironmentVariable, UpdateSettingsArgs,
};
use icp_events::TaskOutcome;

use crate::operations::task::{Reporter, Task};
use snafu::{ResultExt, Snafu};
use tracing::{error, warn};

use super::proxy::UpdateOrProxyError;
use super::proxy_management;

#[derive(Debug, Snafu)]
pub enum BindingEnvVarsOperationError {
    #[snafu(display("Could not find canister id(s) for {} in environment '{environment}'. Make sure they are created first", canister_names.join(", ")))]
    CanisterNotCreated {
        environment: String,
        canister_names: Vec<String>,
    },

    #[snafu(display("failed to fetch current canister settings for canister {canister}"))]
    FetchCurrentSettings {
        source: UpdateOrProxyError,
        canister: Principal,
    },

    #[snafu(transparent)]
    UpdateOrProxy { source: UpdateOrProxyError },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to update environment variables."))]
pub struct SetBindingEnvVarsManyError {
    names: Vec<String>,
}

/// The environment-variable namespace the project's bindings are stamped into.
const BINDING_PREFIX: &str = "PUBLIC_CANISTER_ID:";

/// Write a canister's environment variables: the ones its manifest declares,
/// with the ids it is wired to stamped over them.
///
/// `unresolved` names the bindings this run could not compute, as
/// `(variable, referenced canister)`. Their current values are carried over,
/// because the update below replaces the canister's whole variable list, and an
/// unresolved binding means "no id in *this* store" — the dependency was
/// deployed from another project root, or lies outside this command's scope —
/// not "this canister is not wired to it". Without that, a deploy scoped to one
/// service would delete the id a full workspace deploy had stamped, leaving the
/// canister to read an empty value with nothing said anywhere.
pub async fn set_env_vars_for_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_id: &Principal,
    canister_info: &Canister,
    binding_vars: &[(String, String)],
    unresolved: &[(String, String)],
) -> Result<(), BindingEnvVarsOperationError> {
    let mut environment_variables = canister_info
        .settings
        .environment_variables
        .to_owned()
        .unwrap_or_default();

    // Only an unresolved binding needs the canister's current state, so the
    // common path still writes without reading first.
    if !unresolved.is_empty() {
        let status = proxy_management::canister_status(
            agent,
            proxy,
            CanisterIdRecord {
                canister_id: *canister_id,
            },
        )
        .await
        .context(FetchCurrentSettingsSnafu {
            canister: *canister_id,
        })?;

        for (variable, _) in unresolved {
            let Some(current) = status
                .settings
                .environment_variables
                .iter()
                .find(|v| &v.name == variable)
            else {
                continue;
            };
            // A value the manifest declares still outranks a stamped one.
            environment_variables
                .entry(variable.to_owned())
                .or_insert_with(|| current.value.clone());
        }
    }

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
        let task = reporter.task(Task::update_environment_variables(info.name.clone(), cid));

        // Each canister receives only the ids it is wired to (its own project's
        // canisters by their local names, plus any declared dependencies under
        // their aliases), resolved to the ids that exist in this environment.
        // A project without dependencies wires every canister to every sibling,
        // reproducing the previous flat behavior.
        //
        // A binding whose target has no id in this environment is kept aside
        // rather than dropped: the canister may already carry the value, and
        // silently writing it away is how a scoped deploy used to unwire a
        // canister from its dependency.
        let mut binding_vars: Vec<(String, String)> = Vec::new();
        let mut unresolved: Vec<(String, String)> = Vec::new();
        for (env_name, referenced_key) in &info.bindings {
            let variable = format!("{BINDING_PREFIX}{env_name}");
            match canister_list.get(referenced_key) {
                Some(principal) => binding_vars.push((variable, principal.to_text())),
                None => unresolved.push((variable, referenced_key.to_owned())),
            }
        }

        let agent = agent.clone();
        futs.push_back(async move {
            for (variable, referenced_key) in &unresolved {
                warn!(
                    "Canister '{}' is wired to '{referenced_key}', which has no id in environment \
                     '{environment_name}'; leaving '{variable}' as it is. Deploy \
                     '{referenced_key}' to stamp its id.",
                    info.name
                );
            }

            let result =
                set_env_vars_for_canister(&agent, proxy, &cid, &info, &binding_vars, &unresolved)
                    .await;

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
