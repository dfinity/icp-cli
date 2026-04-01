use std::time::{SystemTime, UNIX_EPOCH};

use candid::CandidType;
use ic_agent::export::Principal;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::{fs, prelude::*};

/// Matches the Candid `DelegationChain` record from the cli-backend canister.
/// All byte fields are hex-encoded strings on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, CandidType)]
pub struct DelegationChain {
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub delegations: Vec<SignedDelegation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, CandidType)]
pub struct SignedDelegation {
    pub signature: String,
    pub delegation: Delegation,
}

#[derive(Debug, Clone, Serialize, Deserialize, CandidType)]
pub struct Delegation {
    pub pubkey: String,
    pub expiration: String,
    pub targets: Option<Vec<String>>,
}

/// Convert a [`DelegationChain`] from the Candid wire format (hex strings) into
/// the ic-agent types used by [`ic_agent::identity::DelegatedIdentity`].
///
/// Returns `(from_key, delegations)` where `from_key` is the DER-encoded root
/// public key of the delegation chain.
pub fn to_agent_types(
    chain: &DelegationChain,
) -> Result<(Vec<u8>, Vec<ic_agent::identity::SignedDelegation>), ConversionError> {
    let from_key =
        hex::decode(&chain.public_key).context(InvalidHexSnafu { field: "publicKey" })?;

    let delegations = chain
        .delegations
        .iter()
        .map(|sd| {
            let signature =
                hex::decode(&sd.signature).context(InvalidHexSnafu { field: "signature" })?;

            let pubkey =
                hex::decode(&sd.delegation.pubkey).context(InvalidHexSnafu { field: "pubkey" })?;

            let expiration = u64::from_str_radix(&sd.delegation.expiration, 16).context(
                InvalidExpirationSnafu {
                    value: &sd.delegation.expiration,
                },
            )?;

            let targets = sd
                .delegation
                .targets
                .as_ref()
                .map(|ts| {
                    ts.iter()
                        .map(|t| {
                            let bytes =
                                hex::decode(t).context(InvalidHexSnafu { field: "targets" })?;
                            Ok(Principal::from_slice(&bytes))
                        })
                        .collect::<Result<Vec<_>, ConversionError>>()
                })
                .transpose()?;

            Ok(ic_agent::identity::SignedDelegation {
                delegation: ic_agent::identity::Delegation {
                    pubkey,
                    expiration,
                    targets,
                },
                signature,
            })
        })
        .collect::<Result<Vec<_>, ConversionError>>()?;

    Ok((from_key, delegations))
}

/// Returns the earliest expiration (nanoseconds since epoch) across all
/// delegations in the chain.
pub fn earliest_expiration(chain: &DelegationChain) -> Result<u64, ConversionError> {
    chain
        .delegations
        .iter()
        .map(|sd| {
            u64::from_str_radix(&sd.delegation.expiration, 16).context(InvalidExpirationSnafu {
                value: &sd.delegation.expiration,
            })
        })
        .try_fold(u64::MAX, |acc, exp| Ok(acc.min(exp?)))
}

/// Returns `true` if the delegation chain has already expired or will expire
/// within `grace_nanos` nanoseconds from now.
pub fn is_expiring_soon(
    chain: &DelegationChain,
    grace_nanos: u64,
) -> Result<bool, ConversionError> {
    let earliest = earliest_expiration(chain)?;
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos() as u64;
    Ok(earliest <= now_nanos.saturating_add(grace_nanos))
}

pub fn load(path: &Path) -> Result<DelegationChain, LoadError> {
    Ok(fs::json::load(path)?)
}

pub fn save(path: &Path, chain: &DelegationChain) -> Result<(), SaveError> {
    fs::json::save(path, chain)?;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum ConversionError {
    #[snafu(display("invalid hex in delegation field `{field}`"))]
    InvalidHex {
        field: String,
        source: hex::FromHexError,
    },

    #[snafu(display("invalid expiration timestamp `{value}`"))]
    InvalidExpiration {
        value: String,
        source: std::num::ParseIntError,
    },
}

#[derive(Debug, Snafu)]
pub enum LoadError {
    #[snafu(transparent)]
    Json { source: fs::json::Error },
}

#[derive(Debug, Snafu)]
pub enum SaveError {
    #[snafu(transparent)]
    Json { source: fs::json::Error },
}
