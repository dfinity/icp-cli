use clap::Parser;
use ic_agent::export::Principal;

use crate::{
    commands::Context,
    commands::{
        build,
        canister::{
            binding_env_vars,
            create::{self, CanisterSettings},
            install,
        },
        sync,
    },
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Parser, Debug)]
pub struct Cmd {
    /// Canister names
    pub names: Vec<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub mode: String,

    /// The subnet id to use for the canisters being deployed.
    #[clap(long)]
    pub subnet_id: Option<Principal>,

    /// One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub controller: Vec<Principal>,

    /// Cycles to fund canister creation (in cycles).
    #[arg(long, default_value_t = create::DEFAULT_CANISTER_CYCLES)]
    pub cycles: u128,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
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

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await?;

    // Load target environment
    let env =
        p.environments
            .get(cmd.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: cmd.environment.name().to_owned(),
            })?;

    let cnames = match cmd.names.is_empty() {
        // No canisters specified
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => cmd.names,
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
        build::Cmd {
            names: cnames.to_owned(),
        },
    )
    .await?;

    // Create the selected canisters
    let _ = ctx.term.write_line("\n\nCreating canisters:");
    let out = create::exec(
        ctx,
        create::Cmd {
            names: cnames.to_owned(),
            identity: cmd.identity.clone(),
            environment: cmd.environment.clone(),

            // Controllers
            controller: cmd.controller.to_owned(),

            // Settings
            settings: CanisterSettings {
                ..Default::default()
            },

            quiet: false,
            cycles: cmd.cycles,
            subnet: cmd.subnet_id,
        },
    )
    .await;

    if let Err(err) = out
        && !matches!(err, create::CommandError::NoCanisters) {
            return Err(err.into());
        }

    let _ = ctx.term.write_line("\n\nSetting environment variables:");
    let out = binding_env_vars::exec(
        ctx,
        binding_env_vars::Cmd {
            names: cnames.to_owned(),
            identity: cmd.identity.clone(),
            environment: cmd.environment.clone(),
        },
    )
    .await;

    if let Err(err) = out
        && !matches!(err, binding_env_vars::CommandError::NoCanisters) {
            return Err(err.into());
        }

    // Install the selected canisters
    let _ = ctx.term.write_line("\n\nInstalling canisters:");
    let out = install::exec(
        ctx,
        install::Cmd {
            names: cnames.to_owned(),
            mode: cmd.mode.to_owned(),
            identity: cmd.identity.clone(),
            environment: cmd.environment.clone(),
        },
    )
    .await;

    if let Err(err) = out
        && !matches!(err, install::CommandError::NoCanisters) {
            return Err(err.into());
        }

    // Sync the selected canisters
    let _ = ctx.term.write_line("\n\nSyncing canisters:");
    let out = sync::exec(
        ctx,
        sync::Cmd {
            names: cnames.to_owned(),
            identity: cmd.identity.clone(),
            environment: cmd.environment.clone(),
        },
    )
    .await;

    if let Err(err) = out
        && !matches!(err, sync::CommandError::NoCanisters) {
            return Err(err.into());
        }

    Ok(())
}
