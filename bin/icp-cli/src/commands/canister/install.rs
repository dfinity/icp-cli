use std::{collections::HashSet, sync::Arc};

use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::builders::CanisterInstallMode;
use icp_adapter::script::{ScriptAdapterProgress, ScriptAdapterProgressHandler};
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    progress::ProgressManager,
    store_artifact::LookupError as LookupArtifactError,
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Clone, Debug, Parser)]
pub struct Cmd {
    /// The names of the canisters within the current project
    pub names: Vec<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub mode: String,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project()?;

    // Choose canisters to install
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.names.is_empty() {
            // If no names specified, create all canisters
            true => true,

            // If names specified, only create matching canisters
            false => cmd.names.contains(&c.name),
        })
        .collect::<Vec<_>>();

    // Check if selected canisters exists
    if !cmd.names.is_empty() {
        let names = cs.iter().map(|(_, c)| &c.name).collect::<HashSet<_>>();

        for name in &cmd.names {
            if !names.contains(name) {
                return Err(CommandError::CanisterNotFound {
                    name: name.to_owned(),
                });
            }
        }
    }

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Filter for environment canisters
    let cs = cs
        .iter()
        .filter(|(_, c)| ecs.contains(&c.name))
        .collect::<Vec<_>>();

    // Ensure canister is included in the environment
    if !cmd.names.is_empty() {
        for name in &cmd.names {
            if !ecs.contains(name) {
                return Err(CommandError::EnvironmentCanister {
                    environment: env.name.to_owned(),
                    canister: name.to_owned(),
                });
            }
        }
    }

    // Ensure at least one canister has been selected
    if cs.is_empty() {
        return Err(CommandError::NoCanisters);
    }

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Prepare a futures set for concurrent operations
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    for (_, c) in cs {
        let sph = Arc::new(progress_manager.new_progress_handler(c.name.clone()));

        // Create an async closure that handles the operation for this specific canister
        let install_fn = {
            let cmd = cmd.clone();
            let mgmt = mgmt.clone();
            let sph = sph.clone();

            async move {
                // Indicate to user that the canister is being installed
                sph.progress_update(ScriptAdapterProgress::ScriptStarted {
                    title: "Installing...".to_string(),
                });

                // Lookup the canister id
                let cid = ctx.id_store.lookup(&Key {
                    network: network.to_owned(),
                    environment: env.name.to_owned(),
                    canister: c.name.to_owned(),
                })?;

                // Lookup the canister build artifact
                let wasm = ctx.artifact_store.lookup(&c.name)?;

                // Retrieve canister status
                let (status,) = mgmt.canister_status(&cid).await?;

                let install_mode = match cmd.mode.as_ref() {
                    // Auto
                    "auto" => match status.module_hash {
                        // Canister has had code installed to it.
                        Some(_) => CanisterInstallMode::Upgrade(None),

                        // Canister has not had code installed to it.
                        None => CanisterInstallMode::Install,
                    },

                    // Install
                    "install" => CanisterInstallMode::Install,

                    // Reinstall
                    "reinstall" => CanisterInstallMode::Reinstall,

                    // Upgrade
                    "upgrade" => CanisterInstallMode::Upgrade(None),

                    // invalid
                    _ => panic!("invalid install mode"),
                };

                // Install code to canister
                mgmt.install_code(&cid, &wasm)
                    .with_mode(install_mode)
                    .await?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the install function with progress tracking
            let result = install_fn.await;
            match result {
                Ok(_) => sph.progress_update(ScriptAdapterProgress::ScriptFinished {
                    status: true,
                    title: "Created".to_string(),
                }),
                Err(e) => {
                    sph.progress_update(ScriptAdapterProgress::ScriptFinished {
                        status: false,
                        title: format!("Installation failed: {}", e),
                    });
                    return Err(e);
                }
            }
            result
        });
    }

    // Consume the set of futures and abort if an error occurs
    while let Some(res) = futs.next().await {
        // TODO(or.ricon): Handle canister creation failures
        res?;
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to install"))]
    NoCanisters,

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    LookupCanisterArtifact { source: LookupArtifactError },

    #[snafu(transparent)]
    InstallAgent { source: AgentError },
}
