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

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_delegation(expiration: String, targets: Option<Vec<String>>) -> SignedDelegation {
        SignedDelegation {
            signature: "0a0b0c".to_string(),
            delegation: Delegation {
                pubkey: "01020304".to_string(),
                expiration,
                targets,
            },
        }
    }

    fn chain_with_delegations(delegations: Vec<SignedDelegation>) -> DelegationChain {
        DelegationChain {
            public_key: "a1b2c3d4".to_string(),
            delegations,
        }
    }

    #[test]
    fn to_agent_types_decodes_fields_and_targets() {
        let target = Principal::from_slice(&[1, 2, 3, 4]);
        let chain = chain_with_delegations(vec![signed_delegation(
            format!("{:x}", 0x1234_u64),
            Some(vec![hex::encode(target.as_slice())]),
        )]);

        let (from_key, delegations) = to_agent_types(&chain).expect("conversion should succeed");

        assert_eq!(from_key, vec![0xa1, 0xb2, 0xc3, 0xd4]);
        assert_eq!(delegations.len(), 1);
        assert_eq!(delegations[0].signature, vec![0x0a, 0x0b, 0x0c]);
        assert_eq!(delegations[0].delegation.pubkey, vec![0x01, 0x02, 0x03, 0x04]);
        assert_eq!(delegations[0].delegation.expiration, 0x1234);
        assert_eq!(
            delegations[0].delegation.targets.as_ref().expect("targets"),
            &vec![target]
        );
    }

    #[test]
    fn to_agent_types_rejects_invalid_public_key_hex() {
        let chain = DelegationChain {
            public_key: "not-hex".to_string(),
            delegations: vec![],
        };

        assert!(to_agent_types(&chain).is_err());
    }

    #[test]
    fn to_agent_types_rejects_invalid_target_hex() {
        let chain = chain_with_delegations(vec![signed_delegation(
            format!("{:x}", 0x1234_u64),
            Some(vec!["xyz".to_string()]),
        )]);

        assert!(to_agent_types(&chain).is_err());
    }

    #[test]
    fn to_agent_types_rejects_invalid_expiration_hex() {
        let chain = chain_with_delegations(vec![signed_delegation(
            "invalid-expiration".to_string(),
            None,
        )]);

        assert!(to_agent_types(&chain).is_err());
    }

    #[test]
    fn earliest_expiration_returns_smallest_expiration() {
        let chain = chain_with_delegations(vec![
            signed_delegation(format!("{:x}", 2_000_000_000_u64), None),
            signed_delegation(format!("{:x}", 1_000_000_000_u64), None),
        ]);

        let earliest = earliest_expiration(&chain)
            .expect("expiration parsing should succeed")
            .expect("expected an expiration");

        assert_eq!(
            earliest,
            UNIX_EPOCH + std::time::Duration::from_nanos(1_000_000_000)
        );
    }

    #[test]
    fn earliest_expiration_rejects_invalid_expiration_hex() {
        let chain = chain_with_delegations(vec![signed_delegation("not-hex".to_string(), None)]);

        assert!(earliest_expiration(&chain).is_err());
    }

    #[test]
    fn is_expiring_soon_is_true_for_past_expiration() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos() as u64;
        let chain = chain_with_delegations(vec![signed_delegation(
            format!("{:x}", now.saturating_sub(1)),
            None,
        )]);

        assert!(is_expiring_soon(&chain).expect("expiration parsing should succeed"));
    }

    #[test]
    fn is_expiring_soon_is_false_for_far_future_expiration() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos() as u64;
        let chain = chain_with_delegations(vec![signed_delegation(
            format!("{:x}", now.saturating_add(365 * 24 * 60 * 60 * 1_000_000_000_u64)),
            None,
        )]);

        assert!(!is_expiring_soon(&chain).expect("expiration parsing should succeed"));
    }

    #[test]
    fn is_expiring_soon_rejects_invalid_expiration_hex() {
        let chain = chain_with_delegations(vec![signed_delegation("bad-expiration".to_string(), None)]);

        assert!(is_expiring_soon(&chain).is_err());
    }
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
