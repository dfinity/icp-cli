use clap::Args;

use crate::{
    commands::{Context, Mode, args},
    options::EnvironmentOpt,
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    pub(crate) canister: args::Canister,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("an invalid argument was provided")]
    Args,

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    LookupCanisterId(#[from] LookupIdError),
}

pub(crate) async fn exec(ctx: &Context, args: &ShowArgs) -> Result<(), CommandError> {
    match &ctx.mode {
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

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

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

            println!("{cid} => {}", name);

            // TODO(or.ricon): Show canister details
            //  Things we might want to show (do we need to sub-command this?)
            //  - canister manifest (e.g resulting canister manifest after recipe definitions are processed)
            //  - canister deployment details (this canister is deployed to network X as part of environment Y)
        }
    }

    Ok(())
}
