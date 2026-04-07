use clap::Args;
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection},
    identity::{
        IdentitySelection, key,
        manifest::{IdentityList, IdentitySpec},
    },
};
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
    let algorithm = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            let list = IdentityList::load_from(dirs)?;
            let spec = list
                .identities
                .get(&args.name)
                .context(IdentityNotFoundSnafu { name: &args.name })?;
            match spec {
                IdentitySpec::InternetIdentity { algorithm, .. } => Ok(algorithm.clone()),
                _ => NotIiSnafu { name: &args.name }.fail(),
            }
        })
        .await??;

    let der_public_key =
        key::load_ii_session_public_key(&args.name, &algorithm).context(LoadSessionKeySnafu)?;

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

    #[snafu(display("failed to load II session key from keyring"))]
    LoadSessionKey { source: key::LoadIdentityError },

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
