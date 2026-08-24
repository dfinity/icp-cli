use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use candid::{Nat, Principal};
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::Agent;
use ic_management_canister_types::{
    CanisterIdRecord, CanisterSettings, EnvironmentVariable, UpdateSettingsArgs,
};
use icp::{
    Canister,
    canister::{Settings, Visibility, resolve_controllers},
    context::{Context, EnvironmentSelection},
    store_id::IdMapping,
};
use itertools::Itertools;
use num_traits::ToPrimitive;
use snafu::{ResultExt, Snafu};
use tracing::{error, warn};

use crate::progress::{ProgressManager, ProgressManagerSettings};

use super::proxy::UpdateOrProxyError;
use super::proxy_management;

#[derive(Debug, Snafu)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SyncSettingsOperationError {
    #[snafu(display("failed to fetch current canister settings for canister {canister}"))]
    FetchCurrentSettings {
        source: UpdateOrProxyError,
        canister: Principal,
    },
    #[snafu(display("failed to update canister settings for canister {canister}"))]
    UpdateSettings {
        source: UpdateOrProxyError,
        canister: Principal,
    },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to update settings."))]
pub struct SyncSettingsManyError {
    names: Vec<String>,
}

/// Holds error information from a failed canister settings update operation
struct SettingsFailure {
    canister_name: String,
    canister_id: Principal,
    error: SyncSettingsOperationError,
}

/// Compare two visibility settings, treating the allowed-viewers list as a set
/// so a reordering from the replica does not look like a pending change.
fn visibility_eq(a: &Visibility, b: &Visibility) -> bool {
    match (a, b) {
        (Visibility::Controllers, Visibility::Controllers) => true,
        (Visibility::Public, Visibility::Public) => true,
        (Visibility::AllowedViewers(va), Visibility::AllowedViewers(vb)) => {
            let set_a: HashSet<_> = va.iter().collect();
            let set_b: HashSet<_> = vb.iter().collect();
            set_a == set_b
        }
        _ => false,
    }
}

/// Compare two environment variable lists in an order-insensitive manner.
/// Uses HashMap comparison (by name -> value).
fn environment_variables_eq(a: &[EnvironmentVariable], b: &[EnvironmentVariable]) -> bool {
    let map_a: HashMap<_, _> = a.iter().map(|ev| (&ev.name, &ev.value)).collect();
    let map_b: HashMap<_, _> = b.iter().map(|ev| (&ev.name, &ev.value)).collect();
    map_a == map_b
}

/// Syncs the manifest settings to the canister. Returns names of any controller canister
/// references that could not be resolved because the referenced canister has not been created
/// yet. Resolved controllers are always applied immediately.
pub(crate) async fn sync_settings(
    agent: &Agent,
    proxy: Option<Principal>,
    cid: &Principal,
    canister: &Canister,
    ids: &IdMapping,
) -> Result<Vec<String>, SyncSettingsOperationError> {
    let status =
        proxy_management::canister_status(agent, proxy, CanisterIdRecord { canister_id: *cid })
            .await
            .context(FetchCurrentSettingsSnafu { canister: *cid })?;
    let &Settings {
        ref log_visibility,
        ref status_visibility,
        compute_allocation,
        ref memory_allocation,
        ref freezing_threshold,
        ref reserved_cycles_limit,
        ref wasm_memory_limit,
        ref wasm_memory_threshold,
        ref log_memory_limit,
        ref environment_variables,
        ref controllers,
    } = &canister.settings;
    let current_settings = status.settings;

    let desired_log_visibility = log_visibility.clone().map(Visibility::from);
    let desired_status_visibility = status_visibility.clone().map(Visibility::from);
    let current_log_visibility = Visibility::from(current_settings.log_visibility.clone());
    let current_status_visibility = Visibility::from(current_settings.status_visibility.clone());

    let environment_variable_setting =
        if let Some(configured_environment_variables) = &environment_variables {
            let mut merged_environment_variables: HashMap<_, _> = current_settings
                .environment_variables
                .clone()
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

    // Resolve controller references. Append-only: union resolved principals with the current
    // list so existing controllers are never removed. Unresolved canister names are returned
    // to the caller as warnings.
    let (controllers_setting, unresolved_names): (Option<Vec<Principal>>, Vec<String>) =
        if let Some(crefs) = controllers {
            let (resolved, unresolved) = resolve_controllers(crefs, ids);
            // Start from the existing controller list and add any new principals.
            let mut merged = current_settings.controllers.clone();
            for p in resolved {
                if !merged.contains(&p) {
                    merged.push(p);
                }
            }
            (Some(merged), unresolved)
        } else {
            (None, vec![])
        };

    let controllers_need_update = controllers_setting.as_ref().is_some_and(|desired| {
        let mut desired_sorted = desired.clone();
        desired_sorted.sort();
        let mut current_sorted = current_settings.controllers.clone();
        current_sorted.sort();
        desired_sorted != current_sorted
    });

    if desired_log_visibility
        .as_ref()
        .is_none_or(|s| visibility_eq(s, &current_log_visibility))
        && desired_status_visibility
            .as_ref()
            .is_none_or(|s| visibility_eq(s, &current_status_visibility))
        && compute_allocation.is_none_or(|s| s == current_settings.compute_allocation)
        && memory_allocation
            .as_ref()
            .map(|m| m.get())
            .is_none_or(|s| current_settings.memory_allocation.0.to_u64() == Some(s))
        && freezing_threshold
            .as_ref()
            .map(|d| d.get())
            .is_none_or(|s| s == current_settings.freezing_threshold)
        && reserved_cycles_limit
            .as_ref()
            .is_none_or(|s| s.get() == current_settings.reserved_cycles_limit)
        && wasm_memory_limit
            .as_ref()
            .map(|m| m.get())
            .is_none_or(|s| current_settings.wasm_memory_limit.0.to_u64() == Some(s))
        && wasm_memory_threshold
            .as_ref()
            .map(|m| m.get())
            .is_none_or(|s| current_settings.wasm_memory_threshold.0.to_u64() == Some(s))
        && log_memory_limit
            .as_ref()
            .map(|m| m.get())
            .is_none_or(|s| current_settings.log_memory_limit.0.to_u64() == Some(s))
        && environment_variable_setting
            .as_ref()
            .is_none_or(|s| environment_variables_eq(s, &current_settings.environment_variables))
        && !controllers_need_update
    {
        // No changes needed
        return Ok(unresolved_names);
    }

    let settings = CanisterSettings {
        log_visibility: desired_log_visibility.map(Into::into),
        status_visibility: desired_status_visibility.map(Into::into),
        compute_allocation: compute_allocation.map(Nat::from),
        memory_allocation: memory_allocation.as_ref().map(|m| Nat::from(m.get())),
        freezing_threshold: freezing_threshold.as_ref().map(|d| Nat::from(d.get())),
        reserved_cycles_limit: reserved_cycles_limit.as_ref().map(|r| Nat::from(r.get())),
        wasm_memory_limit: wasm_memory_limit.as_ref().map(|m| Nat::from(m.get())),
        wasm_memory_threshold: wasm_memory_threshold.as_ref().map(|m| Nat::from(m.get())),
        log_memory_limit: log_memory_limit.as_ref().map(|m| Nat::from(m.get())),
        environment_variables: environment_variable_setting,
        controllers: controllers_setting,
        // Not configurable from the manifest yet; `None` leaves them unchanged.
        snapshot_visibility: None,
        minimum_incoming_canister_call_cycles: None,
    };

    proxy_management::update_settings(
        agent,
        proxy,
        UpdateSettingsArgs {
            canister_id: *cid,
            settings,
            sender_canister_version: None,
        },
    )
    .await
    .context(UpdateSettingsSnafu { canister: *cid })?;

    Ok(unresolved_names)
}

pub(crate) async fn sync_settings_many(
    agent: Agent,
    proxy: Option<Principal>,
    target_canisters: Vec<(Principal, Canister)>,
    ids: IdMapping,
    debug: bool,
) -> Result<(), SyncSettingsManyError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });
    let ids = Arc::new(ids);

    for (cid, info) in target_canisters {
        let pb = progress_manager.create_progress_bar(&info.name);
        let canister_name = info.name.clone();
        let ids = ids.clone();

        let settings_fn = {
            let agent = agent.clone();
            let pb = pb.clone();

            async move {
                pb.set_message("Updating canister settings...");
                let unresolved = sync_settings(&agent, proxy, &cid, &info, &ids).await?;
                for name in &unresolved {
                    warn!(
                        "Controller canister '{name}' for '{}' has not been created yet; \
                         it will be set as a controller once created.",
                        info.name
                    );
                }
                Ok::<_, SyncSettingsOperationError>(())
            }
        };

        futs.push_back(async move {
            let result = ProgressManager::execute_with_progress(
                &pb,
                settings_fn,
                || "Canister settings updated successfully".to_string(),
                |err| format!("Failed to update canister settings: {err}"),
            )
            .await;

            // Map error to include canister context for deferred printing
            result.map_err(|error| SettingsFailure {
                canister_name,
                canister_id: cid,
                error,
            })
        });
    }

    // Consume the set of futures and collect errors
    let mut errors: Vec<SettingsFailure> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(failure) = res {
            errors.push(failure);
        }
    }

    if !errors.is_empty() {
        // Print all errors in batch
        for failure in &errors {
            error!(
                "----- Failed to update settings for canister '{}': {} -----",
                failure.canister_name, failure.canister_id,
            );
            error!("'{}'", failure.error);
        }

        return SyncSettingsManySnafu {
            names: errors
                .iter()
                .map(|e| e.canister_name.clone())
                .collect::<Vec<String>>(),
        }
        .fail();
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum SyncControllerDependentsError {
    #[snafu(display("failed to load environment for controller dependent sync"))]
    GetEnvironment {
        source: icp::context::GetEnvironmentError,
    },

    #[snafu(display("failed to load canister IDs for controller dependent sync"))]
    GetIds {
        source: icp::context::GetIdsByEnvironmentError,
    },
}

/// After `newly_created_name` is registered, scan the project manifest for all other canisters
/// that list `newly_created_name` as a controller and already have a stored ID. Calls
/// `sync_settings` for each so the controller is applied now that it can be resolved.
pub(crate) async fn sync_controller_dependents(
    ctx: &Context,
    agent: &Agent,
    proxy: Option<Principal>,
    newly_created_name: &str,
    env: &EnvironmentSelection,
) -> Result<(), SyncControllerDependentsError> {
    let env_data = ctx
        .get_environment(env)
        .await
        .context(GetEnvironmentSnafu)?;
    let ids = ctx.ids_by_environment(env).await.context(GetIdsSnafu)?;

    for (name, (_, canister)) in &env_data.canisters {
        let references_new = canister.settings.controllers.as_ref().is_some_and(|crefs| {
            crefs
                .iter()
                .any(|c| c.canister_name() == Some(newly_created_name))
        });
        if !references_new {
            continue;
        }
        let Some(&cid) = ids.get(name) else {
            continue;
        };
        match sync_settings(agent, proxy, &cid, canister, &ids).await {
            Ok(unresolved) => {
                for still_unresolved in &unresolved {
                    warn!(
                        "Controller canister '{still_unresolved}' for '{name}' has not been \
                         created yet; it will be set as a controller once created."
                    );
                }
            }
            Err(e) => {
                warn!("Failed to apply pending controller update for canister '{name}': {e}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_eq_controllers() {
        assert!(visibility_eq(
            &Visibility::Controllers,
            &Visibility::Controllers
        ));
    }

    #[test]
    fn visibility_eq_public() {
        assert!(visibility_eq(&Visibility::Public, &Visibility::Public));
    }

    #[test]
    fn visibility_eq_different_variants() {
        assert!(!visibility_eq(
            &Visibility::Controllers,
            &Visibility::Public
        ));
        assert!(!visibility_eq(
            &Visibility::Public,
            &Visibility::Controllers
        ));
    }

    #[test]
    fn visibility_eq_allowed_viewers_same_order() {
        let p1 = Principal::from_text("aaaaa-aa").unwrap();
        let p2 = Principal::from_text("2vxsx-fae").unwrap();

        assert!(visibility_eq(
            &Visibility::AllowedViewers(vec![p1, p2]),
            &Visibility::AllowedViewers(vec![p1, p2])
        ));
    }

    #[test]
    fn visibility_eq_allowed_viewers_different_order() {
        let p1 = Principal::from_text("aaaaa-aa").unwrap();
        let p2 = Principal::from_text("2vxsx-fae").unwrap();

        // Order should not matter
        assert!(visibility_eq(
            &Visibility::AllowedViewers(vec![p1, p2]),
            &Visibility::AllowedViewers(vec![p2, p1])
        ));
    }

    #[test]
    fn visibility_eq_allowed_viewers_different_principals() {
        let p1 = Principal::from_text("aaaaa-aa").unwrap();
        let p2 = Principal::from_text("2vxsx-fae").unwrap();
        let p3 = Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap();

        assert!(!visibility_eq(
            &Visibility::AllowedViewers(vec![p1, p2]),
            &Visibility::AllowedViewers(vec![p1, p3])
        ));
    }

    #[test]
    fn visibility_eq_allowed_viewers_different_length() {
        let p1 = Principal::from_text("aaaaa-aa").unwrap();
        let p2 = Principal::from_text("2vxsx-fae").unwrap();

        assert!(!visibility_eq(
            &Visibility::AllowedViewers(vec![p1]),
            &Visibility::AllowedViewers(vec![p1, p2])
        ));
    }

    #[test]
    fn visibility_eq_allowed_viewers_vs_other() {
        let p1 = Principal::from_text("aaaaa-aa").unwrap();

        assert!(!visibility_eq(
            &Visibility::AllowedViewers(vec![p1]),
            &Visibility::Controllers
        ));
        assert!(!visibility_eq(
            &Visibility::AllowedViewers(vec![p1]),
            &Visibility::Public
        ));
    }

    #[test]
    fn environment_variables_eq_same_order() {
        let vars1 = vec![
            EnvironmentVariable {
                name: "A".to_string(),
                value: "1".to_string(),
            },
            EnvironmentVariable {
                name: "B".to_string(),
                value: "2".to_string(),
            },
        ];
        let vars2 = vec![
            EnvironmentVariable {
                name: "A".to_string(),
                value: "1".to_string(),
            },
            EnvironmentVariable {
                name: "B".to_string(),
                value: "2".to_string(),
            },
        ];

        assert!(environment_variables_eq(&vars1, &vars2));
    }

    #[test]
    fn environment_variables_eq_different_order() {
        let vars1 = vec![
            EnvironmentVariable {
                name: "A".to_string(),
                value: "1".to_string(),
            },
            EnvironmentVariable {
                name: "B".to_string(),
                value: "2".to_string(),
            },
        ];
        let vars2 = vec![
            EnvironmentVariable {
                name: "B".to_string(),
                value: "2".to_string(),
            },
            EnvironmentVariable {
                name: "A".to_string(),
                value: "1".to_string(),
            },
        ];

        // Order should not matter
        assert!(environment_variables_eq(&vars1, &vars2));
    }

    #[test]
    fn environment_variables_eq_different_values() {
        let vars1 = vec![EnvironmentVariable {
            name: "A".to_string(),
            value: "1".to_string(),
        }];
        let vars2 = vec![EnvironmentVariable {
            name: "A".to_string(),
            value: "2".to_string(),
        }];

        assert!(!environment_variables_eq(&vars1, &vars2));
    }

    #[test]
    fn environment_variables_eq_different_keys() {
        let vars1 = vec![EnvironmentVariable {
            name: "A".to_string(),
            value: "1".to_string(),
        }];
        let vars2 = vec![EnvironmentVariable {
            name: "B".to_string(),
            value: "1".to_string(),
        }];

        assert!(!environment_variables_eq(&vars1, &vars2));
    }

    #[test]
    fn environment_variables_eq_different_length() {
        let vars1 = vec![EnvironmentVariable {
            name: "A".to_string(),
            value: "1".to_string(),
        }];
        let vars2 = vec![
            EnvironmentVariable {
                name: "A".to_string(),
                value: "1".to_string(),
            },
            EnvironmentVariable {
                name: "B".to_string(),
                value: "2".to_string(),
            },
        ];

        assert!(!environment_variables_eq(&vars1, &vars2));
    }

    #[test]
    fn environment_variables_eq_empty() {
        let vars1: Vec<EnvironmentVariable> = vec![];
        let vars2: Vec<EnvironmentVariable> = vec![];

        assert!(environment_variables_eq(&vars1, &vars2));
    }
}
