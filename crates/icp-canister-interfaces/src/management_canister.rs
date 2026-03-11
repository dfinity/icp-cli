use candid::{CandidType, Nat, Principal};
use serde::Deserialize;

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct MgmtCreateCanisterArgs {
    pub settings: Option<CanisterSettingsArg>,
    pub sender_canister_version: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct MgmtCreateCanisterResponse {
    pub canister_id: Principal,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct CanisterSettingsArg {
    pub freezing_threshold: Option<Nat>,
    pub controllers: Option<Vec<Principal>>,
    pub reserved_cycles_limit: Option<Nat>,
    pub log_visibility: Option<LogVisibility>,
    pub memory_allocation: Option<Nat>,
    pub compute_allocation: Option<Nat>,
}

/// Log visibility setting for a canister.
/// Matches the cycles ledger's LogVisibility variant type.
#[derive(Clone, Debug, CandidType, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogVisibility {
    Controllers,
    Public,
    AllowedViewers(Vec<Principal>),
}
