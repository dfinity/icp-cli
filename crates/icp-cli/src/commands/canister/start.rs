use anyhow::{Context as _, anyhow};
use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};

use crate::{
    commands::{
        Context, Mode, args,
        validation::{self, Validate, ValidateError},
    },
    impl_from_args,
    options::IdentityOpt,
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Args)]
pub(crate) struct StartArgs {
    pub(crate) canister: args::Canister,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[arg(long)]
    pub(crate) network: Option<args::Network>,

    #[arg(long)]
    pub(crate) environment: Option<args::Environment>,
}

impl_from_args!(StartArgs, canister: args::Canister);
impl_from_args!(StartArgs, network: Option<args::Network>);
impl_from_args!(StartArgs, environment: Option<args::Environment>);
impl_from_args!(StartArgs, network: Option<args::Network>, environment: Option<args::Environment>);

impl Validate for StartArgs {
    fn validate(&self, mode: &Mode) -> Result<(), ValidateError> {
        for test in [
            validation::a_canister_id_is_required_in_global_mode,
            validation::a_network_url_is_required_in_global_mode,
            validation::environments_are_not_available_in_a_global_mode,
            validation::network_or_environment_not_both,
        ] {
            test(self, mode)
                .map(|msg| anyhow::format_err!(msg))
                .map_or(Ok(()), Err)?;
        }

        Ok(())
    }
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
            // Argument (Canister)
            let args::Canister::Principal(cid) = &args.canister else {
                return Err(CommandError::Args);
            };

            // Argument (Network)
            let Some(args::Network::Url(url)) = args.network.clone() else {
                return Err(CommandError::Args);
            };

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Agent
            let agent = ctx.agent.create(id, &url).await?;

            (agent, cid.to_owned())
        }

        Mode::Project(pdir) => {
            // Argument (Canister)
            let args::Canister::Name(name) = &args.canister else {
                return Err(CommandError::Args);
            };

            // Argument (Environment)
            let args::Environment::Name(env) = args.environment.clone().unwrap_or_default();

            // Load project
            let p = ctx.project.load(pdir).await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p
                .environments
                .get(&env)
                .ok_or(CommandError::EnvironmentNotFound { name: env })?;

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
