//! Interface to the control-panel **engine-canister** — the registry of cloud
//! engines, which maps an engine's subnet to its per-engine **engine-operator**
//! canister and knows which engines a caller may see.
//!
//! On a `SubnetType::CloudEngine` subnet, canister creation is delegated to the
//! subnet's engine-operator (which exposes a cycles-ledger-compatible
//! `create_canister`). To find that operator the CLI asks the engine-canister
//! "what is the engine-operator id for this subnet?" via
//! [`GET_ENGINE_OPERATOR_BY_SUBNET_METHOD`].
//!
//! The argument and result of that query are dedicated `opt`-field wrapper
//! records (`…Args` / `…Result`), mirroring the control-panel convention so the
//! interface can grow fields on either side without breaking the wire format.
//!
//! The CLI also lets a user name an engine instead of its subnet. That is
//! answered by [`LIST_VISIBLE_ENGINES_METHOD`], the engines the caller may see,
//! and [`GET_ENGINE_METADATA_METHOD`], an engine's human-facing name.

use std::env::{self, VarError};

use candid::{CandidType, Principal};
use serde::Deserialize;

/// Default engine-canister principal: the registry on mainnet.
///
/// Overridable at runtime via the [`ENGINE_CANISTER_ID_ENV`] environment
/// variable — see [`engine_canister_id`].
pub const ENGINE_CANISTER_CID: &str = "q6cfj-fyaaa-aaaar-qb77q-cai";

/// Environment variable that overrides [`ENGINE_CANISTER_CID`].
pub const ENGINE_CANISTER_ID_ENV: &str = "ENGINE_CANISTER_ID";

/// The engine-canister query that resolves a subnet to its engine-operator id.
pub const GET_ENGINE_OPERATOR_BY_SUBNET_METHOD: &str = "getEngineOperatorBySubnet";

/// The engine-canister query that lists the engines the caller may see:
/// `() -> (vec Engine)`.
pub const LIST_VISIBLE_ENGINES_METHOD: &str = "listVisibleEngines";

/// The engine-canister query that returns an engine's metadata by id:
/// `(EngineId) -> (MetadataResult)`.
pub const GET_ENGINE_METADATA_METHOD: &str = "getEngineMetadata";

/// Resolve the engine-canister principal to talk to.
///
/// Uses the value of the `ENGINE_CANISTER_ID` environment variable when set and
/// non-empty, otherwise falls back to [`ENGINE_CANISTER_CID`]. Returns an error
/// when the override is set but invalid — either not a valid principal, or not
/// valid Unicode. Only an absent or empty (whitespace-only) variable uses the
/// default: a configured-but-invalid value must never silently route to the
/// built-in registry (and thus a different environment).
pub fn engine_canister_id() -> Result<Principal, String> {
    resolve_engine_canister_id(env::var(ENGINE_CANISTER_ID_ENV))
}

/// [`engine_canister_id`] with the environment read out, so the precedence
/// can be tested without touching the process environment.
fn resolve_engine_canister_id(env: Result<String, VarError>) -> Result<Principal, String> {
    match env {
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

/// One engine, as [`LIST_VISIBLE_ENGINES_METHOD`] reports it.
///
/// Only the fields the CLI reads are named here; Candid lets the canister
/// report more. An engine's human-facing name is not among them — that is a
/// separate [`GET_ENGINE_METADATA_METHOD`] query by `id`.
#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Engine {
    /// The engine's id, the key every other engine-canister method takes.
    pub id: String,
    /// The principal that owns the engine.
    pub owner: Principal,
    /// The subnet the engine runs. `None` until the engine's subnet exists.
    pub subnet_id: Option<Principal>,
    /// When the engine was deleted. A deleted engine may still be listed.
    pub deleted_at: Option<i64>,
    /// The per-engine engine-operator canister. `None` until it is recorded.
    pub engine_operator_id: Option<Principal>,
}

/// An engine's human-facing metadata, as [`GET_ENGINE_METADATA_METHOD`]
/// reports it.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct EngineMetadata {
    /// The engine's name. Not unique on its own: the canister enforces
    /// uniqueness of the pair (`name`, `slug`).
    pub name: String,
    /// A short caller-supplied disambiguator for engines sharing a name.
    pub slug: String,
}

/// Why an engine-canister method refused.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum EngineError {
    NotFound,
    Unauthorized,
    NotVetted,
    AlreadyExists,
    SlugConflict(String),
    BadRequest(String),
    ControllerError(String),
    InvalidTransition(String),
    Internal(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::Unauthorized => write!(f, "unauthorized"),
            Self::NotVetted => write!(f, "not vetted"),
            Self::AlreadyExists => write!(f, "already exists"),
            Self::SlugConflict(message) => write!(f, "slug conflict: {message}"),
            Self::BadRequest(message) => write!(f, "bad request: {message}"),
            Self::ControllerError(message) => write!(f, "controller error: {message}"),
            Self::InvalidTransition(message) => write!(f, "invalid transition: {message}"),
            Self::Internal(message) => write!(f, "internal error: {message}"),
        }
    }
}

/// Result of [`GET_ENGINE_METADATA_METHOD`].
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum MetadataResult {
    #[serde(rename = "ok")]
    Ok(EngineMetadata),
    #[serde(rename = "err")]
    Err(EngineError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_engine_canister_id_parses() {
        // The built-in default must always be a valid principal.
        assert_eq!(
            resolve_engine_canister_id(Err(VarError::NotPresent)).unwrap(),
            Principal::from_text(ENGINE_CANISTER_CID).unwrap()
        );
        // An empty variable is as good as an absent one.
        assert_eq!(
            resolve_engine_canister_id(Ok("  ".to_string())).unwrap(),
            Principal::from_text(ENGINE_CANISTER_CID).unwrap()
        );
    }

    #[test]
    fn environment_overrides_default_engine_canister() {
        let other = "rrkah-fqaaa-aaaaa-aaaaq-cai";
        assert_eq!(
            resolve_engine_canister_id(Ok(other.to_string())).unwrap(),
            Principal::from_text(other).unwrap()
        );
    }

    #[test]
    fn invalid_environment_override_is_an_error() {
        let err = resolve_engine_canister_id(Ok("not-a-principal".to_string())).unwrap_err();
        assert!(err.contains(ENGINE_CANISTER_ID_ENV), "{err}");
    }
}
