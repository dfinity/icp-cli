use clap::Parser;
use ic_agent::export::Principal;
use icp_identity::key::LoadIdentityInContextError;
use snafu::Snafu;

use crate::{
    commands::{
        build,
        canister::{
            create::{self, CanisterIDs, CanisterSettings, DEFAULT_EFFECTIVE_ID},
            install,
        },
        sync,
    },
    context::{Context, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The name of the canister within the current project
    name: Option<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub mode: String,

    #[clap(flatten)]
    pub identity: IdentityOpt,

    #[clap(flatten)]
    pub environment: EnvironmentOpt,

    /// The effective canister ID to use when calling the management canister.
    #[clap(long, default_value = DEFAULT_EFFECTIVE_ID)]
    pub effective_id: Principal,

    /// One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple.
    #[clap(long)]
    pub controller: Vec<Principal>,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Choose canisters to create
    let canisters = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Check if a canister name was specified and is present in the project
    if let Some(name) = &cmd.name {
        if canisters.is_empty() {
            return Err(CommandError::CanisterNotFound {
                name: name.to_owned(),
            });
        }
    }

    // Prepare canister names for subsequent commands
    let cnames = canisters
        .iter()
        .map(|(_, c)| c.name.to_owned())
        .collect::<Vec<_>>();

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

            // Ids
            ids: CanisterIDs {
                effective_id: cmd.effective_id.to_owned(),
                specific_id: None,
            },

            // Controllers
            controller: cmd.controller.to_owned(),

            // Settings
            settings: CanisterSettings {
                ..Default::default()
            },

            quiet: false,
        },
    )
    .await;

    if let Err(err) = out {
        if !matches!(err, create::CommandError::NoCanisters) {
            return Err(CommandError::Create { source: err });
        }
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

    if let Err(err) = out {
        if !matches!(err, install::CommandError::NoCanisters) {
            return Err(CommandError::Install { source: err });
        }
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

    if let Err(err) = out {
        if !matches!(err, sync::CommandError::NoCanisters) {
            return Err(CommandError::Sync { source: err });
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    Build { source: build::CommandError },

    #[snafu(transparent)]
    Create { source: create::CommandError },

    #[snafu(transparent)]
    Install { source: install::CommandError },

    #[snafu(transparent)]
    Sync { source: sync::CommandError },
}
