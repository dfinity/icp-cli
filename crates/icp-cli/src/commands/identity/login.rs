use clap::Args;
use ic_agent::Identity as _;
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection},
    identity::{
        IdentitySelection, key,
        manifest::{IdentityKeyAlgorithm, IdentityList, IdentitySpec},
    },
};
use pem::Pem;
use pkcs8::DecodePrivateKey as _;
use sec1::pem::PemLabel as _;
use snafu::{OptionExt, ResultExt, Snafu};
use tracing::info;

use crate::{operations::ii_poll, options::EnvironmentOpt};

/// Re-authenticate an Internet Identity delegation
#[derive(Debug, Args)]
pub(crate) struct LoginArgs {
    /// Name of the identity to re-authenticate
    name: String,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &LoginArgs) -> Result<(), LoginError> {
    let environment: EnvironmentSelection = args.environment.clone().into();

    // Load the identity list and verify this is an II identity
    let (_algorithm, der_public_key) =
        ctx.dirs
            .identity()?
            .with_read(async |dirs| {
                let list = IdentityList::load_from(dirs)?;
                let spec = list
                    .identities
                    .get(&args.name)
                    .context(IdentityNotFoundSnafu { name: &args.name })?;

                let algorithm = match spec {
                    IdentitySpec::InternetIdentity { algorithm, .. } => algorithm.clone(),
                    _ => return NotIiSnafu { name: &args.name }.fail(),
                };

                // Load the existing PEM to get the public key
                let pem_path = dirs.key_pem_path(&args.name);
                let origin = key::PemOrigin::File {
                    path: pem_path.clone(),
                };
                let doc = icp::fs::read_to_string(&pem_path)?
                    .parse::<Pem>()
                    .map_err(|e| LoginError::ParsePem {
                        origin: origin.clone(),
                        source: Box::new(e),
                    })?;

                assert!(
                    doc.tag() == pkcs8::PrivateKeyInfo::PEM_LABEL,
                    "II identity PEM should be plaintext"
                );

                let der_public_key = match algorithm {
                    IdentityKeyAlgorithm::Ed25519 => {
                        let key = ic_ed25519::PrivateKey::deserialize_pkcs8(doc.contents())
                            .map_err(|e| LoginError::ParseKey {
                                origin: origin.clone(),
                                source: Box::new(e),
                            })?;
                        let basic =
                            ic_agent::identity::BasicIdentity::from_raw_key(&key.serialize_raw());
                        basic.public_key().expect("ed25519 always has a public key")
                    }
                    IdentityKeyAlgorithm::Secp256k1 => {
                        let key = k256::SecretKey::from_pkcs8_der(doc.contents()).map_err(|e| {
                            LoginError::ParseKey {
                                origin: origin.clone(),
                                source: Box::new(e),
                            }
                        })?;
                        let id = ic_agent::identity::Secp256k1Identity::from_private_key(key);
                        id.public_key().expect("secp256k1 always has a public key")
                    }
                    IdentityKeyAlgorithm::Prime256v1 => {
                        let key = p256::SecretKey::from_pkcs8_der(doc.contents()).map_err(|e| {
                            LoginError::ParseKey {
                                origin: origin.clone(),
                                source: Box::new(e),
                            }
                        })?;
                        let id = ic_agent::identity::Prime256v1Identity::from_private_key(key);
                        id.public_key().expect("p256 always has a public key")
                    }
                };

                Ok((algorithm, der_public_key))
            })
            .await??;

    // Resolve the environment to get network access
    let env = ctx
        .get_environment(&environment)
        .await
        .context(GetEnvSnafu)?;
    let network_access = ctx
        .network
        .access(&env.network)
        .await
        .context(NetworkAccessSnafu)?;

    let http_gateway_url = network_access
        .http_gateway_url
        .as_ref()
        .context(NoHttpGatewaySnafu)?;

    // Create an anonymous agent for polling
    let agent = ctx
        .get_agent_for_env(&IdentitySelection::Anonymous, &environment)
        .await
        .context(CreateAgentSnafu)?;

    // Look up the cli-backend canister ID
    let delegator_backend_id = ctx
        .get_canister_id_for_env(
            &CanisterSelection::Named("backend".to_string()),
            &environment,
        )
        .await
        .context(LookupCanisterSnafu)?;
    let delegator_frontend_id = ctx
        .get_canister_id_for_env(
            &CanisterSelection::Named("frontend".to_string()),
            &environment,
        )
        .await
        .context(LookupCanisterSnafu)?;

    let delegator_frontend_friendly = if network_access.use_friendly_domains {
        Some(("frontend", env.name.as_str()))
    } else {
        None
    };

    // Open browser and poll for delegation
    let chain = ii_poll::poll_for_delegation(
        &agent,
        delegator_backend_id,
        delegator_frontend_id,
        &der_public_key,
        http_gateway_url,
        delegator_frontend_friendly,
    )
    .await
    .context(PollSnafu)?;

    // Update the delegation chain
    ctx.dirs
        .identity()?
        .with_write(async |dirs| key::update_ii_delegation(dirs, &args.name, &chain))
        .await?
        .context(UpdateDelegationSnafu)?;

    info!("Identity `{}` re-authenticated", args.name);

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum LoginError {
    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(transparent)]
    LoadManifest {
        source: icp::identity::manifest::LoadIdentityManifestError,
    },

    #[snafu(display("no identity found with name `{name}`"))]
    IdentityNotFound { name: String },

    #[snafu(display(
        "identity `{name}` is not an Internet Identity; use `icp identity link ii` instead"
    ))]
    NotIi { name: String },

    #[snafu(transparent)]
    ReadFile { source: icp::fs::IoError },

    #[snafu(display("failed to parse PEM from `{origin}`"))]
    ParsePem {
        origin: key::PemOrigin,
        #[snafu(source(from(pem::PemError, Box::new)))]
        source: Box<pem::PemError>,
    },

    #[snafu(display("failed to parse key from `{origin}`"))]
    ParseKey {
        origin: key::PemOrigin,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[snafu(display("failed to resolve environment"))]
    GetEnv {
        source: icp::context::GetEnvironmentError,
    },

    #[snafu(display("failed to access network"))]
    NetworkAccess { source: icp::network::AccessError },

    #[snafu(display("network has no HTTP gateway URL configured"))]
    NoHttpGateway,

    #[snafu(display("failed to create agent"))]
    CreateAgent {
        source: icp::context::GetAgentForEnvError,
    },

    #[snafu(display("failed to look up cli-backend canister ID"))]
    LookupCanister {
        source: icp::context::GetCanisterIdForEnvError,
    },

    #[snafu(display("failed during II authentication polling"))]
    Poll { source: ii_poll::IiPollError },

    #[snafu(display("failed to update delegation"))]
    UpdateDelegation {
        source: key::UpdateIiDelegationError,
    },
}
