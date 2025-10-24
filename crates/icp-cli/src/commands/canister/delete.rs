use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};

use crate::{
    commands::{Context, Mode},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    /// The name of the canister within the current project
    pub(crate) name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
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
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Delete(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
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
            if !env.canisters.contains_key(&args.name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: args.name.to_owned(),
                });
            }

            // Lookup the canister id
            let cid = ctx.ids.lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: args.name.to_owned(),
            })?;

            // Management Interface
            let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

            // Instruct management canister to delete canister
            mgmt.delete_canister(&cid).await?;

            // TODO(or.ricon): Remove the canister association with the network/environment
        }
    }

    Ok(())
}
