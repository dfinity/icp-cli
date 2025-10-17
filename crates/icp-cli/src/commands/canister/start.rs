use anyhow::{Context as _, anyhow};
use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};

use crate::{
    commands::{Context, Mode, args},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Args)]
pub(crate) struct StartArgs {
    pub(crate) canister: args::Canister,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("an invalid argument was provided")]
    Args,

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    LookupCanisterId(#[from] LookupIdError),

    #[error(transparent)]
    Start(#[from] AgentError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &StartArgs) -> Result<(), CommandError> {
    let (agent, cid) = match &ctx.mode {
        Mode::Global => {
            let args::Canister::Principal(_) = &args.canister else {
                return Err(CommandError::Args);
            };

            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            let args::Canister::Name(name) = &args.canister else {
                return Err(CommandError::Args);
            };

            // Load project
            let p = ctx.project.load().await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            // Ensure canister is included in the environment
            if !env.canisters.contains_key(name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: name.to_owned(),
                });
            }

            // Lookup the canister id
            let cid = ctx.ids.lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            })?;

            (agent, cid)
        }
    };

    (ctx.ops.canister.start)(&agent)
        .start(&cid)
        .await
        .context(anyhow!("failed to start canister {cid}"))?;

    Ok(())
}
