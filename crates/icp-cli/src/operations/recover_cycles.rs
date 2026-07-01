//! Recover a canister's liquid cycles before `icp canister delete` burns them.
//!
//! Deleting a canister destroys its remaining cycles. To avoid this we
//! force-install the tiny [`recover-cycles-canister`](crate::artifacts::get_recover_cycles_wasm)
//! module (which deposits the canister's liquid cycles to a destination account
//! on the cycles ledger), invoke it, then let the caller proceed with deletion.
//!
//! Failure handling matches the deliberate policy: a balance too small to
//! recover is a non-fatal warning (those cycles would burn anyway), while a
//! failure to move a *meaningful* balance is a hard error so the user can
//! retry rather than silently lose cycles.

use candid::{CandidType, Decode, Encode, Nat, Principal};
use ic_agent::Agent;
use ic_management_canister_types::{
    CanisterId, CanisterIdRecord, CanisterInstallMode, CanisterSettings, InstallCodeArgs,
    UpdateSettingsArgs,
};
use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use tracing::{info, warn};

use super::proxy::UpdateOrProxyError;
use crate::{artifacts, operations::proxy_management};

/// Total cycle balance (from `canister_status`) at or below which recovery is
/// skipped as not worth the reinstall. Kept equal to RESERVE_MARGIN: a canister
/// at this balance would have roughly nothing liquid to deposit once call
/// overhead is subtracted.
const RECOVERY_THRESHOLD: u128 = 50_000_000_000;

#[derive(Debug, Snafu)]
pub(crate) enum RecoverCyclesError {
    #[snafu(display("failed to query status of canister {canister_id} before cycle recovery"))]
    QueryStatus {
        canister_id: Principal,
        source: UpdateOrProxyError,
    },

    #[snafu(display(
        "failed to lower freezing threshold on canister {canister_id} before cycle recovery"
    ))]
    UpdateSettings {
        canister_id: Principal,
        source: UpdateOrProxyError,
    },

    #[snafu(display("failed to encode the recovery destination principal"))]
    EncodeDestination { source: candid::Error },

    #[snafu(display("failed to install the cycle-recovery module on canister {canister_id}"))]
    InstallModule {
        canister_id: Principal,
        source: UpdateOrProxyError,
    },

    #[snafu(display("failed to start canister {canister_id} for cycle recovery"))]
    StartCanister {
        canister_id: Principal,
        source: UpdateOrProxyError,
    },

    #[snafu(display("failed to encode the recover_cycles arguments"))]
    EncodeArgs { source: candid::Error },

    #[snafu(display("the recover_cycles call to canister {canister_id} failed"))]
    CallRecover {
        canister_id: Principal,
        source: ic_agent::AgentError,
    },

    #[snafu(display("failed to decode the recover_cycles result from canister {canister_id}"))]
    DecodeResult {
        canister_id: Principal,
        source: candid::Error,
    },

    #[snafu(display("canister {canister_id} failed to deposit its cycles: {reason}"))]
    DepositFailed {
        canister_id: Principal,
        reason: String,
    },
}

/// Mirror of the recovery canister's return type (see
/// `recover-cycles-canister/src/lib.rs`).
#[derive(CandidType, Deserialize)]
enum RecoverResult {
    /// Cycles deposited to the destination; carries the amount attached.
    Deposited(u128),
    /// Liquid balance was below the reserve margin — nothing meaningful to move.
    NothingToRecover,
    /// A non-trivial balance failed to deposit; carries the failure reason.
    Failed(String),
}

/// Best-effort cycle recovery before deletion.
///
/// Returns `Ok(())` when recovery succeeded or was harmlessly skipped (balance
/// below the threshold / nothing liquid to move). Returns `Err` only when a
/// canister with a meaningful balance failed to have its cycles recovered, so
/// the caller can abort the delete.
///
/// `proxy` (the controller, when set) is used for the management hops
/// (install/start). The `recover_cycles` invocation itself is always a direct
/// call; the deposit destination is baked into the install argument, so routing
/// does not affect where the cycles land.
pub(crate) async fn recover_cycles_before_delete(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_id: Principal,
    destination: Principal,
) -> Result<(), RecoverCyclesError> {
    let status = proxy_management::canister_status(agent, proxy, CanisterIdRecord { canister_id })
        .await
        .context(QueryStatusSnafu { canister_id })?;
    let cycles: u128 = status.cycles.0.try_into().unwrap_or(u128::MAX);

    if cycles <= RECOVERY_THRESHOLD {
        warn!(
            "Canister {canister_id} holds {cycles} cycles, below the recovery threshold; skipping cycle recovery"
        );
        return Ok(());
    }

    // From here the balance is meaningful, so any failure is fatal to the delete.

    // Lower the freezing threshold so the recovery canister sees a larger liquid
    // balance and can deposit close to the full total.
    proxy_management::update_settings(
        agent,
        proxy,
        UpdateSettingsArgs {
            canister_id: CanisterId::from(canister_id),
            settings: CanisterSettings {
                // Lower the freezing threshold to ensure cycles are liquid
                freezing_threshold: Some(600_u128.into()),
                // Reset several settings to default that affect costs
                compute_allocation: Some(0_u128.into()),
                memory_allocation: Some(0_u128.into()),
                wasm_memory_limit: Some(3_221_225_472_u128.into()),
                wasm_memory_threshold: Some(0_u128.into()),
                reserved_cycles_limit: Some(5_000_000_000_000_u128.into()),
                ..Default::default()
            },
            sender_canister_version: None,
        },
    )
    .await
    .context(UpdateSettingsSnafu { canister_id })?;

    let arg = Encode!(&destination).context(EncodeDestinationSnafu)?;
    proxy_management::install_code(
        agent,
        proxy,
        InstallCodeArgs {
            mode: CanisterInstallMode::Reinstall,
            canister_id: CanisterId::from(canister_id),
            wasm_module: artifacts::get_recover_cycles_wasm().to_vec(),
            arg,
            sender_canister_version: None,
        },
    )
    .await
    .context(InstallModuleSnafu { canister_id })?;

    // The canister must be running to accept the recovery update call.
    proxy_management::start_canister(agent, proxy, CanisterIdRecord { canister_id })
        .await
        .context(StartCanisterSnafu { canister_id })?;

    // Direct (non-proxied) call: the destination is the install arg, not the
    // caller, so proxy routing is irrelevant here.
    let bytes = agent
        .update(&canister_id, "recover_cycles")
        .with_arg(Encode!().context(EncodeArgsSnafu)?)
        .call_and_wait()
        .await
        .context(CallRecoverSnafu { canister_id })?;
    let result = Decode!(&bytes, RecoverResult).context(DecodeResultSnafu { canister_id })?;

    match result {
        RecoverResult::Deposited(amount) => {
            info!("Recovered {amount} cycles from canister {canister_id} to {destination}");
            Ok(())
        }
        RecoverResult::NothingToRecover => {
            warn!(
                "Canister {canister_id} had no liquid cycles above the reserve to recover; deleting anyway"
            );
            Ok(())
        }
        RecoverResult::Failed(reason) => DepositFailedSnafu {
            canister_id,
            reason,
        }
        .fail(),
    }
}
