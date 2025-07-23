use crate::commands::canister::create::DEFAULT_EFFECTIVE_ID;
use crate::context::GetProjectError;
use crate::options::{EnvironmentOpt, IdentityOpt};
use crate::{
    commands::{
        build,
        canister::{
            self,
            create::{CanisterCreateCmd, CanisterCreateError, CanisterIDs, CanisterSettings},
            install::{CanisterInstallCmd, CanisterInstallError},
        },
        sync,
    },
    context::Context,
};
use clap::Parser;
use ic_agent::export::Principal;
use icp_identity::key::LoadIdentityInContextError;
use snafu::Snafu;

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

    // Build the selected canisters
    eprintln!("\nBuilding canisters:");
    for (_, c) in &canisters {
        eprintln!("- {}", c.name);

        // TODO(or.ricon): Temporary approach that can be revisited.
        //                 Currently we simply invoke the adjacent `build` command.
        //                 We should consider refactoring `build` to use library code instead.
        build::exec(
            ctx,
            build::Cmd {
                name: Some(c.name.to_owned()),
            },
        )
        .await?;
    }

    // Create the selected canisters
    eprintln!("\nCreating canisters:");
    for (_, c) in &canisters {
        eprintln!("- {}", c.name);

        // TODO(or.ricon): Temporary approach that can be revisited.
        //                 Currently we simply invoke the adjacent `canister::create` command.
        //                 We should consider refactoring `canister::create` to use library code instead.
        let out = canister::create::exec(
            ctx,
            CanisterCreateCmd {
                name: Some(c.name.to_owned()),
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
            if !matches!(err, CanisterCreateError::NoCanisters) {
                return Err(CommandError::Create { source: err });
            }
        }
    }

    // Install the selected canisters
    eprintln!("\nInstalling canisters:");
    for (_, c) in &canisters {
        eprintln!("- {}", c.name);

        // TODO(or.ricon): Temporary approach that can be revisited.
        //                 Currently we simply invoke the adjacent `canister::create` command.
        //                 We should consider refactoring `canister::create` to use library code instead.
        let out = canister::install::exec(
            ctx,
            CanisterInstallCmd {
                name: Some(c.name.to_owned()),
                mode: cmd.mode.to_owned(),
                identity: cmd.identity.clone(),
                environment: cmd.environment.clone(),
            },
        )
        .await;

        if let Err(err) = out {
            if !matches!(err, CanisterInstallError::NoCanisters) {
                return Err(CommandError::Install { source: err });
            }
        }
    }

    // Sync the selected canisters
    eprintln!("\nSyncing canisters:");
    for (_, c) in &canisters {
        eprintln!("- {}", c.name);

        // TODO(or.ricon): Temporary approach that can be revisited.
        //                 Currently we simply invoke the adjacent `canister::sync` command.
        //                 We should consider refactoring `canister::sync` to use library code instead.
        let out = sync::exec(
            ctx,
            sync::Cmd {
                name: Some(c.name.to_owned()),
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
    Create {
        source: canister::create::CanisterCreateError,
    },

    #[snafu(transparent)]
    Install {
        source: canister::install::CanisterInstallError,
    },

    #[snafu(transparent)]
    Sync { source: sync::CommandError },
}
