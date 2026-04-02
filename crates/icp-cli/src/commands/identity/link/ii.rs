use clap::Args;
use ic_agent::{Identity as _, export::Principal, identity::BasicIdentity};
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection},
    identity::{IdentitySelection, key},
};
use snafu::{OptionExt, ResultExt, Snafu};
use tracing::info;

use crate::{commands::identity::ii_poll, options::EnvironmentOpt};

/// Link an Internet Identity to a new identity
#[derive(Debug, Args)]
pub(crate) struct IiArgs {
    /// Name for the linked identity
    name: String,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &IiArgs) -> Result<(), IiError> {
    let environment: EnvironmentSelection = args.environment.clone().into();

    // Generate an Ed25519 keypair for the session key
    let secret_key = ic_ed25519::PrivateKey::generate();
    let identity_key = key::IdentityKey::Ed25519(secret_key.clone());
    let basic = BasicIdentity::from_raw_key(&secret_key.serialize_raw());
    let der_public_key = basic.public_key().expect("ed25519 always has a public key");

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

    // Derive the II principal from the root of the delegation chain
    let from_key = hex::decode(&chain.public_key).context(DecodeFromKeySnafu)?;
    let ii_principal = Principal::self_authenticating(&from_key);

    // Save the identity
    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            key::link_ii_identity(dirs, &args.name, identity_key, &chain, ii_principal)
        })
        .await?
        .context(LinkSnafu)?;

    info!("Identity `{}` linked to Internet Identity", args.name);

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum IiError {
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

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeFromKey { source: hex::FromHexError },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link II identity"))]
    Link { source: key::LinkIiIdentityError },
}
