//! Registers named identities with a local network's Internet Identity, so they exist
//! as soon as the network is up.
//!
//! Local Internet Identity runs with dummy auth: instead of a passkey, its frontend derives an
//! Ed25519 key from a seed index the user enters. Each identity here is registered with exactly
//! the key the frontend derives for its index, so signing in with that index finds it.

use candid::{Decode, Encode};
use ic_agent::{Agent, AgentError, Identity, identity::BasicIdentity};
use icp_canister_interfaces::internet_identity::{
    AuthnMethod, AuthnMethodData, AuthnMethodProtection, AuthnMethodPurpose,
    AuthnMethodSecuritySettings, DeviceKeyWithAnchor, INTERNET_IDENTITY_FRONTEND_CID,
    INTERNET_IDENTITY_PRINCIPAL, IdRegFinishArg, IdRegFinishError, IdRegFinishResult,
    IdRegNextStepResult, IdRegStartError, MetadataEntryV2, RegistrationFlowNextStep, WebAuthn,
};
use snafu::prelude::*;
use tracing::info;
use url::Url;

use crate::network::custom_domains::gateway_domain;

/// An identity registered with Internet Identity, or found already registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IiIdentity {
    pub name: String,
    /// The seed index to enter when the local Internet Identity login asks for one.
    pub index: u64,
    pub identity_number: u64,
}

/// The seed the Internet Identity frontend derives its dummy-auth key from for `index`:
/// the index as a big-endian `u64`, zero-padded to 32 bytes. It doubles as the credential id.
pub fn dummy_auth_seed(index: u64) -> [u8; 32] {
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&index.to_be_bytes());
    seed
}

/// Registers each of `names` with Internet Identity, the name at position `i` under seed index
/// `i`. Names whose key is already registered are left as they are, so this can run on every
/// start of a network that keeps its state.
///
/// Registration is sequential: Internet Identity hands out identity numbers in order, so a fresh
/// network always gives the same names the same numbers.
pub async fn seed(
    api_url: &Url,
    root_key: &[u8],
    names: &[String],
    origin: &str,
) -> Result<Vec<IiIdentity>, SeedIiIdentitiesError> {
    let mut identities = Vec::with_capacity(names.len());
    for (index, name) in (0u64..).zip(names) {
        let identity_number = seed_one(api_url, root_key, name, index, origin).await?;
        identities.push(IiIdentity {
            name: name.clone(),
            index,
            identity_number,
        });
    }
    Ok(identities)
}

async fn seed_one(
    api_url: &Url,
    root_key: &[u8],
    name: &str,
    index: u64,
    origin: &str,
) -> Result<u64, SeedIiIdentitiesError> {
    let seed = dummy_auth_seed(index);
    let identity = BasicIdentity::from_raw_key(&seed);
    let pubkey = identity
        .public_key()
        .expect("an Ed25519 identity always has a public key");
    let agent = Agent::builder()
        .with_url(api_url.as_str())
        .with_identity(identity)
        .build()
        .context(BuildAgentSnafu { name })?;
    agent.set_root_key(root_key.to_vec());

    let response = agent
        .query(&INTERNET_IDENTITY_PRINCIPAL, "lookup_device_key")
        .with_arg(Encode!(&seed.to_vec()).expect("encoding a blob cannot fail"))
        .call()
        .await
        .context(LookupSnafu { name })?;
    let existing =
        Decode!(&response, Option<DeviceKeyWithAnchor>).context(DecodeLookupSnafu { name })?;
    if let Some(existing) = existing {
        return Ok(existing.anchor_number);
    }

    let response = agent
        .update(&INTERNET_IDENTITY_PRINCIPAL, "identity_registration_start")
        .with_arg(Encode!().expect("encoding no arguments cannot fail"))
        .await
        .context(StartRegistrationSnafu { name })?;
    let started = Decode!(&response, Result<IdRegNextStepResult, IdRegStartError>)
        .context(DecodeStartRegistrationSnafu { name })?
        .map_err(|error| SeedIiIdentitiesError::StartRegistrationRejected {
            name: name.to_string(),
            error,
        })?;
    if let RegistrationFlowNextStep::CheckCaptcha { .. } = started.next_step {
        return CaptchaRequiredSnafu { name }.fail();
    }

    let arg = IdRegFinishArg {
        name: Some(name.to_string()),
        authn_method: AuthnMethodData {
            authn_method: AuthnMethod::WebAuthn(WebAuthn {
                pubkey,
                credential_id: seed.to_vec(),
                aaguid: None,
            }),
            security_settings: AuthnMethodSecuritySettings {
                protection: AuthnMethodProtection::Unprotected,
                purpose: AuthnMethodPurpose::Authentication,
            },
            metadata: vec![(
                "origin".to_string(),
                MetadataEntryV2::String(origin.to_string()),
            )],
            last_authentication: None,
        },
    };
    let response = agent
        .update(&INTERNET_IDENTITY_PRINCIPAL, "identity_registration_finish")
        .with_arg(Encode!(&arg).expect("encoding the registration cannot fail"))
        .await
        .context(FinishRegistrationSnafu { name })?;
    let finished = Decode!(&response, Result<IdRegFinishResult, IdRegFinishError>)
        .context(DecodeFinishRegistrationSnafu { name })?
        .map_err(|error| SeedIiIdentitiesError::FinishRegistrationRejected {
            name: name.to_string(),
            error,
        })?;
    Ok(finished.identity_number)
}

/// Logs which seed index signs in as which identity.
pub fn report(origin: &str, identities: &[IiIdentity]) {
    let width = identities.iter().map(|i| i.name.len()).max().unwrap_or(0);
    info!("Sign in to Internet Identity at {origin} with one of these seed indexes:");
    for identity in identities {
        info!(
            "  {:<width$}  index {}  identity {}",
            identity.name, identity.index, identity.identity_number
        );
    }
}

/// The origin the Internet Identity frontend is served from on this network.
pub fn frontend_origin(gateway_url: &Url, port: u16, use_friendly_domains: bool) -> String {
    match gateway_domain(gateway_url) {
        Some(domain) if use_friendly_domains => format!("http://id.ai.{domain}:{port}"),
        _ => format!("http://{INTERNET_IDENTITY_FRONTEND_CID}.localhost:{port}"),
    }
}

#[derive(Debug, Snafu)]
pub enum SeedIiIdentitiesError {
    #[snafu(display("failed to build an agent for Internet Identity identity `{name}`"))]
    BuildAgent {
        name: String,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display("failed to look up Internet Identity identity `{name}`"))]
    Lookup {
        name: String,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display("failed to decode the lookup of Internet Identity identity `{name}`"))]
    DecodeLookup { name: String, source: candid::Error },

    #[snafu(display("failed to start registering Internet Identity identity `{name}`"))]
    StartRegistration {
        name: String,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display(
        "failed to decode the start of registering Internet Identity identity `{name}`"
    ))]
    DecodeStartRegistration { name: String, source: candid::Error },

    #[snafu(display(
        "Internet Identity refused to start registering identity `{name}`: {error:?}"
    ))]
    StartRegistrationRejected {
        name: String,
        error: IdRegStartError,
    },

    #[snafu(display(
        "Internet Identity asks for a captcha to register identity `{name}`, which this network's Internet Identity is not expected to require"
    ))]
    CaptchaRequired { name: String },

    #[snafu(display("failed to finish registering Internet Identity identity `{name}`"))]
    FinishRegistration {
        name: String,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },

    #[snafu(display(
        "failed to decode the result of registering Internet Identity identity `{name}`"
    ))]
    DecodeFinishRegistration { name: String, source: candid::Error },

    #[snafu(display("Internet Identity refused to register identity `{name}`: {error:?}"))]
    FinishRegistrationRejected {
        name: String,
        error: IdRegFinishError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_auth_seed_matches_the_frontend_derivation() {
        assert_eq!(dummy_auth_seed(0), [0u8; 32]);
        let mut expected = [0u8; 32];
        expected[7] = 2;
        assert_eq!(dummy_auth_seed(2), expected);
        let mut expected = [0u8; 32];
        expected[6] = 1;
        assert_eq!(dummy_auth_seed(256), expected);
    }

    #[test]
    fn index_zero_key_is_the_one_the_frontend_signs_in_with() {
        // The key for index 0 that local Internet Identity's frontend signed in with.
        let identity = BasicIdentity::from_raw_key(&dummy_auth_seed(0));
        assert_eq!(
            hex::encode(identity.public_key().unwrap()),
            "302a300506032b65700321003b6a27bcceb6a42d62a3a8d02a6f0d73653215771de243a63ac048a18b59da29"
        );
    }
}
