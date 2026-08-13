use std::collections::{BTreeMap, HashSet};

use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use ic_management_canister_types::{CanisterSettings, EnvironmentVariable, UpdateSettingsArgs};
use icp::Canister;
use snafu::Snafu;
use tracing::error;

use icp_events::{Reporter, TaskKind};

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

/// Holds error information from a failed environment variable update operation
struct BindingEnvVarsFailure {
    canister_name: String,
    canister_id: Principal,
    error: BindingEnvVarsOperationError,
}

pub(crate) async fn set_env_vars_for_canister(
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

/// Orchestrates setting environment variables for multiple canisters with progress tracking
pub(crate) async fn set_binding_env_vars_many(
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
        // Started up front so the tasks appear in the order the canisters were given,
        // regardless of the order the futures below are first polled in.
        let task = reporter.task(TaskKind::Spinner, info.name.as_str());
        let canister_name = info.name.clone();

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

        let settings_fn = {
            let agent = agent.clone();

            async move { set_env_vars_for_canister(&agent, proxy, &cid, &info, &binding_vars).await }
        };

        futs.push_back(async move {
            task.message("Updating environment variables...");

            let result = task
                .run(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::test_support::{
        bare_canister, outcome_of, recording_reporter, task_labels, unreachable_agent,
    };
    use icp_events::{Event, Outcome, TaskId, TaskKind};

    fn canister_id() -> Principal {
        Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap()
    }

    fn ids(names: &[&str]) -> BTreeMap<String, Principal> {
        names
            .iter()
            .map(|name| (name.to_string(), canister_id()))
            .collect()
    }

    /// The agent cannot be reached, so this pins the reporting shape — start,
    /// message, failure verdict — rather than the wording of the transport error.
    #[tokio::test]
    async fn an_unreachable_canister_is_reported_as_a_failure() {
        let (reporter, sink) = recording_reporter();

        let result = set_binding_env_vars_many(
            unreachable_agent(),
            None,
            "default",
            vec![(canister_id(), bare_canister("backend"))],
            ids(&["backend"]),
            &reporter,
        )
        .await;

        assert!(result.is_err());

        let events = sink.events();
        assert_eq!(
            events[..2],
            [
                Event::TaskStarted {
                    id: TaskId(0),
                    kind: TaskKind::Spinner,
                    label: Some("backend".into()),
                },
                Event::TaskMessage {
                    id: TaskId(0),
                    message: "Updating environment variables...".into(),
                },
            ]
        );

        let (outcome, message) = outcome_of(&events, TaskId(0));
        assert_eq!(outcome, Outcome::Failure);
        assert!(
            message
                .as_deref()
                .is_some_and(|m| m.starts_with("Failed to update environment variables: ")),
            "unexpected message: {message:?}"
        );
    }

    #[tokio::test]
    async fn one_task_per_canister_in_the_given_order() {
        let (reporter, sink) = recording_reporter();

        let _ = set_binding_env_vars_many(
            unreachable_agent(),
            None,
            "default",
            vec![
                (canister_id(), bare_canister("frontend")),
                (canister_id(), bare_canister("backend")),
            ],
            ids(&["frontend", "backend"]),
            &reporter,
        )
        .await;

        assert_eq!(
            task_labels(&sink.events()),
            vec![Some("frontend".to_string()), Some("backend".to_string())]
        );
    }

    /// Canisters without ids are rejected before any work starts, so there is
    /// nothing to report progress about.
    #[tokio::test]
    async fn a_canister_without_an_id_aborts_before_any_task_starts() {
        let (reporter, sink) = recording_reporter();

        let result = set_binding_env_vars_many(
            unreachable_agent(),
            None,
            "default",
            vec![(canister_id(), bare_canister("backend"))],
            ids(&[]),
            &reporter,
        )
        .await;

        assert!(result.is_err());
        assert!(sink.events().is_empty());
    }
}
