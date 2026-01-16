use std::collections::HashMap;

use candid::Principal;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError};
use ic_management_canister_types::EnvironmentVariable;
use ic_utils::interfaces::ManagementCanister;
use icp::{Canister, canister::Settings};
use itertools::Itertools;
use snafu::{ResultExt, Snafu};

use crate::progress::{ProgressManager, ProgressManagerSettings};

#[derive(Debug, Snafu)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum SyncSettingsOperationError {
    #[snafu(display("failed to fetch current canister settings for canister {canister}"))]
    FetchCurrentSettings {
        source: AgentError,
        canister: Principal,
    },
    #[snafu(display("invalid canister settings in manifest for canister {name}"))]
    ValidateSettings { source: AgentError, name: String },
    #[snafu(display("failed to update canister settings for canister {canister}"))]
    UpdateSettings {
        source: AgentError,
        canister: Principal,
    },
}

pub(crate) async fn sync_settings(
    mgmt: &ManagementCanister<'_>,
    cid: &Principal,
    canister: &Canister,
) -> Result<(), SyncSettingsOperationError> {
    let (status,) = mgmt
        .canister_status(cid)
        .await
        .context(FetchCurrentSettingsSnafu { canister: *cid })?;
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
    if compute_allocation.is_none_or(|s| s == current_settings.compute_allocation)
        && memory_allocation.is_none_or(|s| s == current_settings.memory_allocation)
        && freezing_threshold.is_none_or(|s| s == current_settings.freezing_threshold)
        && reserved_cycles_limit.is_none_or(|s| s == current_settings.reserved_cycles_limit)
        && wasm_memory_limit.is_none_or(|s| s == current_settings.wasm_memory_limit)
        && wasm_memory_threshold.is_none_or(|s| s == current_settings.wasm_memory_threshold)
        && environment_variable_setting
            .as_ref()
            .is_none_or(|s| *s == current_settings.environment_variables)
    {
        // No changes needed
        return Ok(());
    }
    mgmt.update_settings(cid)
        .with_optional_compute_allocation(compute_allocation)
        .with_optional_memory_allocation(memory_allocation)
        .with_optional_freezing_threshold(freezing_threshold)
        .with_optional_reserved_cycles_limit(reserved_cycles_limit)
        .with_optional_wasm_memory_limit(wasm_memory_limit)
        .with_optional_wasm_memory_threshold(wasm_memory_threshold)
        .with_optional_environment_variables(environment_variable_setting)
        .build()
        .context(ValidateSettingsSnafu {
            name: &canister.name,
        })?
        .await
        .context(UpdateSettingsSnafu { canister: *cid })?;

    Ok(())
}

pub(crate) async fn sync_settings_many(
    agent: Agent,
    target_canisters: Vec<(Principal, Canister)>,
    debug: bool,
) -> Result<(), SyncSettingsOperationError> {
    let mgmt = ManagementCanister::create(&agent);

    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (cid, info) in target_canisters {
        let pb = progress_manager.create_progress_bar(&info.name);

        let settings_fn = {
            let mgmt = mgmt.clone();
            let pb = pb.clone();

            async move {
                pb.set_message("Updating canister settings...");
                sync_settings(&mgmt, &cid, &info).await
            }
        };

        futs.push_back(async move {
            ProgressManager::execute_with_progress(
                &pb,
                settings_fn,
                || "Canister settings updated successfully".to_string(),
                |err| format!("Failed to update canister settings: {err}"),
            )
            .await
        });
    }

    while let Some(res) = futs.next().await {
        res?;
    }

    Ok(())
}
