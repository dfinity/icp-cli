//! Interface to the control-panel **engine-canister** — the registry that maps
//! an engine's subnet to its per-engine **engine-operator** canister.
//!
//! On a `SubnetType::CloudEngine` subnet, canister creation is delegated to the
//! subnet's engine-operator (which exposes a cycles-ledger-compatible
//! `create_canister`). To find that operator the CLI asks the engine-canister
//! "what is the engine-operator id for this subnet?" via
//! [`GET_ENGINE_OPERATOR_BY_SUBNET_METHOD`].
//!
//! The argument and result are dedicated `opt`-field wrapper records
//! (`…Args` / `…Result`), mirroring the control-panel convention so the
//! interface can grow fields on either side without breaking the wire format.

use std::env::{self, VarError};

use candid::{CandidType, Principal};
use serde::Deserialize;

/// Default engine-canister principal (staging/prod registry).
///
/// Overridable at runtime via the [`ENGINE_CANISTER_ID_ENV`] environment
/// variable — see [`engine_canister_id`].
pub const ENGINE_CANISTER_CID: &str = "q6cfj-fyaaa-aaaar-qb77q-cai";

/// Environment variable that overrides [`ENGINE_CANISTER_CID`].
pub const ENGINE_CANISTER_ID_ENV: &str = "ENGINE_CANISTER_ID";

/// The engine-canister query that resolves a subnet to its engine-operator id.
pub const GET_ENGINE_OPERATOR_BY_SUBNET_METHOD: &str = "getEngineOperatorBySubnet";

/// Resolve the engine-canister principal to talk to.
///
/// Uses the value of the `ENGINE_CANISTER_ID` environment variable when set and
/// non-empty, otherwise falls back to [`ENGINE_CANISTER_CID`]. Returns an error
/// when the override is set but invalid — either not a valid principal, or not
/// valid Unicode. Only an absent or empty (whitespace-only) variable uses the
/// default: a configured-but-invalid value must never silently route to the
/// built-in registry (and thus a different environment).
pub fn engine_canister_id() -> Result<Principal, String> {
    match env::var(ENGINE_CANISTER_ID_ENV) {
        Ok(value) if !value.trim().is_empty() => Principal::from_text(value.trim())
            .map_err(|e| format!("invalid {ENGINE_CANISTER_ID_ENV}: {e}")),
        // Set but non-Unicode is still a configured override — reject it rather
        // than silently using the default and targeting the wrong environment.
        Err(VarError::NotUnicode(_)) => Err(format!(
            "invalid {ENGINE_CANISTER_ID_ENV}: not valid Unicode"
        )),
        // Unset or empty/whitespace-only: use the built-in default.
        Ok(_) | Err(VarError::NotPresent) => Ok(Principal::from_text(ENGINE_CANISTER_CID)
            .expect("ENGINE_CANISTER_CID is a valid principal")),
    }
}

/// Argument for [`GET_ENGINE_OPERATOR_BY_SUBNET_METHOD`].
///
/// A single `opt`-field record so the engine-canister can add inputs later
/// without changing the method's candid type.
#[derive(Clone, Debug, Default, CandidType, Deserialize)]
pub struct GetEngineOperatorBySubnetArgs {
    /// The subnet whose engine-operator should be resolved.
    pub subnet_id: Option<Principal>,
}

/// Result of [`GET_ENGINE_OPERATOR_BY_SUBNET_METHOD`].
///
/// `engine_operator_id` is `None` when no live engine is bound to the queried
/// subnet, or the matched engine has no operator recorded yet. The caller
/// treats a `None` here the same as "the subnet does not exist".
#[derive(Clone, Debug, Default, CandidType, Deserialize)]
pub struct GetEngineOperatorBySubnetResult {
    /// The per-engine engine-operator canister id for the subnet, if any.
    pub engine_operator_id: Option<Principal>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_engine_canister_id_parses() {
        // The built-in default must always be a valid principal.
        assert_eq!(
            engine_canister_id().unwrap(),
            Principal::from_text(ENGINE_CANISTER_CID).unwrap()
        );
    }
}
