use clap::Parser;
use ic_agent::{AgentError, export::Principal};
use icp_identity::key::LoadIdentityInContextError;
use snafu::Snafu;

use crate::{
    commands::{
        build,
        canister::{
            binding_env_vars,
            create::{self, CanisterIDs, CanisterSettings, DEFAULT_EFFECTIVE_ID},
            install,
        },
        sync,
    },
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::LookupError,
};

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The name of the canister within the current project
    name: Option<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub mode: String,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,

    /// The subnet id to use for the canisters being deployed.
    #[clap(long)]
    pub subnet_id: Option<Principal>,

    /// One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple.
    #[arg(long)]
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

    // Infer the effective canister id from the subnet id if provided.
    let mut effective_id = Principal::from_text(DEFAULT_EFFECTIVE_ID).unwrap();
    if let Some(subnet_id) = cmd.subnet_id {
        // Load environment
        let env = pm
            .environments
            .iter()
            .find(|&v| v.name == cmd.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: cmd.environment.name().to_owned(),
            })?;

        // Get network
        let network = env
            .network
            .as_ref()
            .expect("no network specified in environment");

        // Load identity
        ctx.require_identity(cmd.identity.name());

        // Setup network
        ctx.require_network(network);

        // Prepare agent
        let agent = ctx.agent()?;

        // Get subnet canister ranges
        let ranges = agent.read_state_subnet_canister_ranges(subnet_id).await?;
        if !ranges.is_empty() {
            effective_id = ranges[0].0 // Use the first start canister id as the effective id.
        }
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
                effective_id: effective_id.to_owned(),
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

    if let Err(err) = out {
        if !matches!(err, binding_env_vars::CommandError::NoCanisters) {
            return Err(CommandError::SetEnvironmentVariables { source: err });
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
    GetProject { source: ContextProjectError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    Build { source: build::CommandError },

    #[snafu(transparent)]
    Create { source: create::CommandError },

    #[snafu(transparent)]
    Install { source: install::CommandError },

    #[snafu(transparent)]
    SetEnvironmentVariables {
        source: binding_env_vars::CommandError,
    },

    #[snafu(transparent)]
    Sync { source: sync::CommandError },

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(transparent)]
    Agent { source: AgentError },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupError },
}
