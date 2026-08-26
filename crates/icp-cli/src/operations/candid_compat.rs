use std::{collections::HashSet, sync::Arc};

use candid::{
    Principal,
    types::subtype::{OptReport, subtype_with_config},
};
use candid_parser::utils::CandidSource;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::Agent;
use ic_management_canister_types::CanisterInstallMode;
use icp_events::{Failure, Reporter, Task, TaskOutcome};
use snafu::Snafu;
use tracing::debug;

use crate::operations::{misc::fetch_canister_metadata, wasm::extract_candid_service};

/// Checks Candid interface compatibility for all canisters that would be
/// upgraded. Aborts if any canister has an incompatible interface.
pub(crate) async fn check_candid_compatibility_many(
    agent: Agent,
    canisters: impl IntoIterator<Item = (&str, Principal, CanisterInstallMode)>,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    reporter: &Reporter,
) -> Result<(), CandidCheckManyError> {
    let mut check_futs = FuturesOrdered::new();

    for (name, cid, mode) in canisters {
        let task = reporter.task(Task::new("Checking Candid compatibility").on(name));
        let is_upgrade = matches!(mode, CanisterInstallMode::Upgrade(_));
        let agent = agent.clone();
        let artifacts = artifacts.clone();

        check_futs.push_back(async move {
            if !is_upgrade {
                task.finish(TaskOutcome::Skipped {
                    reason: "not an upgrade".to_owned(),
                });
                return Ok::<_, String>(());
            }

            let result = check_canister_candid_compat(&agent, &cid, name, &*artifacts).await;

            match &result {
                Ok(()) => task.finish(TaskOutcome::succeeded_with("Compatible")),
                // The incompatibility report is far too long for a progress
                // bar, so the bar gets the verdict and the report is what the
                // deferred dump prints. Both are this check's own words: only
                // it knows a breaking interface change is what went wrong.
                Err(failure) => task.finish(TaskOutcome::Failed(
                    Failure::new("incompatible interface")
                        .with_detail(vec![format!(
                            "You are making a BREAKING change. Other canisters or frontend \
                             clients relying on your canister may stop working.\n\n{}",
                            failure.details,
                        )])
                        .with_epilogue("Use --yes to bypass this check."),
                )),
            }

            result.map_err(|failure| failure.canister_name)
        });
    }

    // Collect the failed canister names; the renderer owns displaying each
    // failure.
    let mut failed: Vec<String> = Vec::new();
    while let Some(res) = check_futs.next().await {
        if let Err(name) = res {
            failed.push(name);
        }
    }

    if !failed.is_empty() {
        return CandidCheckManySnafu { names: failed }.fail();
    }

    Ok(())
}

/// Check candid compatibility for a single canister that is being upgraded.
///
/// Returns `Ok(())` if the check passes or cannot be performed (missing metadata, etc.).
/// Returns `Err(CandidCheckFailure)` only when a genuine incompatibility is found.
async fn check_canister_candid_compat(
    agent: &Agent,
    canister_id: &Principal,
    canister_name: &str,
    artifacts: &dyn icp::store_artifact::Access,
) -> Result<(), CandidCheckFailure> {
    let wasm = match artifacts.lookup(canister_name).await {
        Ok(w) => w,
        // Missing artifact will be caught during install
        Err(_) => return Ok(()),
    };

    match check_candid_compatibility(agent, canister_id, &wasm).await {
        CandidCompatibility::Compatible => Ok(()),
        CandidCompatibility::Skipped(reason) => {
            debug!("Candid compatibility check skipped for {canister_name}: {reason}");
            Ok(())
        }
        CandidCompatibility::Incompatible(details) => Err(CandidCheckFailure {
            canister_name: canister_name.to_owned(),
            details,
        }),
    }
}

/// Check whether the new WASM module's Candid interface is backward-compatible
/// with the currently deployed one.
///
/// Returns [`CandidCompatibility::Skipped`] if either side lacks a
/// `candid:service` metadata section or if the interfaces cannot be parsed.
pub(crate) async fn check_candid_compatibility(
    agent: &Agent,
    canister_id: &Principal,
    wasm: &[u8],
) -> CandidCompatibility {
    // Extract candid:service from the new WASM module
    let new_candid = match extract_candid_service(wasm) {
        Some(s) => s,
        None => {
            return CandidCompatibility::Skipped(
                "new module does not contain candid:service metadata".into(),
            );
        }
    };

    // Fetch candid:service from the deployed canister
    let old_candid = match fetch_canister_metadata(agent, *canister_id, "candid:service").await {
        Some(s) => s,
        None => {
            return CandidCompatibility::Skipped(
                "deployed canister does not expose candid:service metadata".into(),
            );
        }
    };

    // Parse both interfaces and run the subtype check
    let new_loaded = match CandidSource::Text(&new_candid).load() {
        Ok((env, Some(ty))) => (env, ty),
        Ok((_, None)) => {
            return CandidCompatibility::Skipped(
                "new module candid:service does not define a service".into(),
            );
        }
        Err(e) => {
            return CandidCompatibility::Skipped(format!(
                "failed to parse new module candid:service: {e}"
            ));
        }
    };

    let old_loaded = match CandidSource::Text(&old_candid).load() {
        Ok((env, Some(ty))) => (env, ty),
        Ok((_, None)) => {
            return CandidCompatibility::Skipped(
                "deployed candid:service does not define a service".into(),
            );
        }
        Err(e) => {
            return CandidCompatibility::Skipped(format!(
                "failed to parse deployed candid:service: {e}"
            ));
        }
    };

    let (mut env, new_type) = new_loaded;
    let (env2, old_type) = old_loaded;

    let mut gamma = HashSet::new();
    let old_type = env.merge_type(env2, old_type);
    match subtype_with_config(OptReport::Error, &mut gamma, &env, &new_type, &old_type) {
        Ok(()) => CandidCompatibility::Compatible,
        Err(e) => CandidCompatibility::Incompatible(e.to_string()),
    }
}

/// Holds error information from a failed candid compatibility check
struct CandidCheckFailure {
    canister_name: String,
    details: String,
}

/// Result of a Candid interface compatibility check.
pub(crate) enum CandidCompatibility {
    /// Both interfaces present and compatible.
    Compatible,
    /// Both interfaces present but the new one is not a subtype of the old.
    Incompatible(String),
    /// Check could not be performed (missing metadata, parse error, etc.).
    Skipped(String),
}

#[derive(Debug, Snafu)]
#[snafu(display("Candid compatibility check failed for canister(s) {names:?}."))]
pub struct CandidCheckManyError {
    names: Vec<String>,
}
