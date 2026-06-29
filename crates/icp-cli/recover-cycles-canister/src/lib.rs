//! Tiny single-purpose canister force-installed onto a target right before
//! `icp canister delete` destroys it.
//!
//! Deleting a canister burns its remaining cycles, and the management canister
//! offers no way to refund them — the only way out is to run code *inside* the
//! canister that moves the cycles elsewhere first. This canister does exactly
//! that: it deposits its liquid cycle balance to a destination account on the
//! cycles ledger (which mints cycles-ledger tokens equal to the cycles attached
//! to the call), so the cycles end up on the user's account instead of burning.
//!
//! The destination principal is captured at install time, so recovery behaves
//! identically whether or not the invoking call is routed through a proxy.

use candid::{CandidType, Deserialize, Nat, Principal};
use ic_cdk::api::canister_liquid_cycle_balance;
use ic_cdk::call::Call;
use std::cell::RefCell;

/// Cycles ledger: `um5iw-rqaaa-aaaaq-qaaba-cai`.
const CYCLES_LEDGER: Principal = Principal::from_slice(&[0, 0, 0, 0, 2, 16, 0, 2, 1, 1]);

/// Headroom (in cycles) kept on top of the `deposit` call's own transmission
/// cost. `ic-cdk` pre-checks `attached + cost_call(..) <= liquid_balance` before
/// sending, so the only requirement is that this margin exceeds the true
/// `cost_call`; any excess simply burns at delete time, as it would have anyway.
const RESERVE_MARGIN: u128 = 1_000_000_000;

#[derive(CandidType, Deserialize)]
struct Account {
    owner: Principal,
    subaccount: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize)]
struct DepositArgs {
    to: Account,
    memo: Option<Vec<u8>>,
}

#[derive(CandidType, Deserialize)]
struct DepositResult {
    balance: Nat,
    block_index: Nat,
}

/// Outcome reported back to the CLI so it can distinguish "nothing worth
/// recovering" (a warning) from "a real balance failed to move" (an error).
#[derive(CandidType, Deserialize)]
enum RecoverResult {
    /// Cycles deposited to the destination; carries the amount attached.
    Deposited(u128),
    /// Liquid balance was below the reserve margin — nothing meaningful to move.
    NothingToRecover,
    /// A non-trivial balance failed to deposit; carries the failure reason.
    Failed(String),
}

thread_local! {
    /// Destination account owner for recovered cycles, captured at install time.
    static DESTINATION: RefCell<Principal> = const { RefCell::new(Principal::anonymous()) };
}

#[ic_cdk::init]
fn init(destination: Principal) {
    DESTINATION.with(|d| *d.borrow_mut() = destination);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(destination: Principal) {
    init(destination);
}

/// Deposit all liquid cycles (minus the reserve margin) to the configured
/// destination on the cycles ledger. Best-effort: on failure the cycles stay on
/// the canister and are burned at delete time, i.e. the pre-existing behavior.
#[ic_cdk::update]
async fn recover_cycles() -> RecoverResult {
    let liquid = canister_liquid_cycle_balance();
    let to_attach = liquid.saturating_sub(RESERVE_MARGIN);
    if to_attach == 0 {
        return RecoverResult::NothingToRecover;
    }

    let owner = DESTINATION.with(|d| *d.borrow());
    let args = DepositArgs {
        to: Account {
            owner,
            subaccount: None,
        },
        memo: None,
    };

    match Call::unbounded_wait(CYCLES_LEDGER, "deposit")
        .with_arg(args)
        .with_cycles(to_attach)
        .await
    {
        Ok(response) => match response.candid::<DepositResult>() {
            Ok(_) => RecoverResult::Deposited(to_attach),
            Err(e) => RecoverResult::Failed(format!("failed to decode deposit result: {e}")),
        },
        Err(e) => RecoverResult::Failed(format!("deposit call failed: {e}")),
    }
}

// Export the Candid interface so tooling can introspect the canister.
ic_cdk::export_candid!();
