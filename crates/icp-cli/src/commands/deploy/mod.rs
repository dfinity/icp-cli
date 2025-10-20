use clap::Args;
use ic_agent::export::Principal;

use crate::{
    commands::{
        Context, Mode, build,
        canister::{
            binding_env_vars,
            create::{self, CanisterSettings},
            install,
        },
        sync,
    },
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Args, Debug)]
pub(crate) struct DeployArgs {
    /// Canister names
    pub(crate) names: Vec<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// The subnet id to use for the canisters being deployed.
    #[clap(long)]
    pub(crate) subnet_id: Option<Principal>,

    /// One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub(crate) controller: Vec<Principal>,

    /// Cycles to fund canister creation (in cycles).
    #[arg(long, default_value_t = create::DEFAULT_CANISTER_CYCLES)]
    pub(crate) cycles: u128,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Build(#[from] build::CommandError),

    #[error(transparent)]
    Create(#[from] create::CommandError),

    #[error(transparent)]
    Install(#[from] install::CommandError),

    #[error(transparent)]
    SetEnvironmentVariables(#[from] binding_env_vars::CommandError),

    #[error(transparent)]
    Sync(#[from] sync::CommandError),
}

pub(crate) async fn exec(ctx: &Context, args: &DeployArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            // Load the project manifest, which defines the canisters to be built.
            let p = ctx.project.load().await?;

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

            let cnames = match args.names.is_empty() {
                // No canisters specified
                true => env.canisters.keys().cloned().collect(),

                // Individual canisters specified
                false => args.names.clone(),
            };

            for name in &cnames {
                if !p.canisters.contains_key(name) {
                    return Err(CommandError::CanisterNotFound {
                        name: name.to_owned(),
                    });
                }

                if !env.canisters.contains_key(name) {
                    return Err(CommandError::EnvironmentCanister {
                        environment: env.name.to_owned(),
                        canister: name.to_owned(),
                    });
                }
            }

            // Skip doing any work if no canisters are targeted
            if cnames.is_empty() {
                return Ok(());
            }

            // Build the selected canisters
            let _ = ctx.term.write_line("Building canisters:");
            build::exec(
                ctx,
                &build::BuildArgs {
                    names: cnames.to_owned(),
                },
            )
            .await?;

            // Create the selected canisters
            let _ = ctx.term.write_line("\n\nCreating canisters:");
            let out = create::exec(
                ctx,
                &create::CreateArgs {
                    names: cnames.to_owned(),
                    identity: args.identity.clone(),
                    environment: args.environment.clone(),

                    // Controllers
                    controller: args.controller.to_owned(),

                    // Settings
                    settings: CanisterSettings {
                        ..Default::default()
                    },

                    quiet: false,
                    cycles: args.cycles,
                    subnet: args.subnet_id,
                },
            )
            .await;

            if let Err(err) = out
                && !matches!(err, create::CommandError::NoCanisters)
            {
                return Err(err.into());
            }

            let _ = ctx.term.write_line("\n\nSetting environment variables:");
            let out = binding_env_vars::exec(
                ctx,
                &binding_env_vars::BindingArgs {
                    names: cnames.to_owned(),
                    identity: args.identity.clone(),
                    environment: args.environment.clone(),
                },
            )
            .await;

            if let Err(err) = out
                && !matches!(err, binding_env_vars::CommandError::NoCanisters)
            {
                return Err(err.into());
            }

            // Install the selected canisters
            let _ = ctx.term.write_line("\n\nInstalling canisters:");
            let out = install::exec(
                ctx,
                &install::InstallArgs {
                    names: cnames.to_owned(),
                    mode: args.mode.to_owned(),
                    identity: args.identity.clone(),
                    environment: args.environment.clone(),
                },
            )
            .await;

            if let Err(err) = out
                && !matches!(err, install::CommandError::NoCanisters)
            {
                return Err(err.into());
            }

            // Sync the selected canisters
            let _ = ctx.term.write_line("\n\nSyncing canisters:");
            let out = sync::exec(
                ctx,
                &sync::SyncArgs {
                    names: cnames.to_owned(),
                    identity: args.identity.clone(),
                    environment: args.environment.clone(),
                },
            )
            .await;

            if let Err(err) = out
                && !matches!(err, sync::CommandError::NoCanisters)
            {
                return Err(err.into());
            }
        }
    }

    Ok(())
}
