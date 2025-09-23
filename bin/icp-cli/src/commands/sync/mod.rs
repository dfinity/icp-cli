use std::collections::HashSet;

use crate::{
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ScriptProgressHandler},
    store_id::{Key, LookupError},
};
use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use icp_adapter::sync::{Adapter, AdapterSyncError};
use icp_canister::SyncStep;
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The names of the canisters within the current project
    pub names: Vec<String>,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be synced.
    let pm = ctx.project()?;

    // Choose canisters to sync
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.names.is_empty() {
            // If no names specified, sync all canisters
            true => true,

            // If names specified, only sync matching canisters
            false => cmd.names.contains(&c.name),
        })
        .cloned()
        .collect::<Vec<_>>();

    // Check if selected canister exists
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

    // Verify at least one canister is available to sync
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

    // Prepare a futures set for concurrent canister syncs
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    // Iterate through each resolved canister and trigger its sync process.
    for (canister_path, c) in cs {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Get canister principal ID
        let cid = ctx.id_store.lookup(&Key {
            network: network.to_owned(),
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        })?;

        // Create an async closure that handles the sync process for this specific canister
        let sync_fn = {
            let pb = pb.clone();

            async move {
                for step in &c.sync.steps {
                    // Indicate to user the current step being executed
                    let pb_hdr = format!("Syncing: {step}");

                    let script_handler = ScriptProgressHandler::new(pb.clone(), pb_hdr.clone());

                    match step {
                        // Synchronize the canister using the custom script adapter.
                        SyncStep::Script(adapter) => {
                            // Setup script progress handling
                            let tx = script_handler.setup_output_handler();

                            adapter
                                .with_stdio_sender(tx)
                                .sync(canister_path, &cid, agent)
                                .await?
                        }

                        // Synchronize the canister using the assets adapter.
                        SyncStep::Assets(adapter) => {
                            pb.set_message(pb_hdr);
                            adapter.sync(canister_path, &cid, agent).await?
                        }
                    };
                }

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the sync function with progress tracking
            ProgressManager::execute_with_progress(
                pb,
                sync_fn,
                || format!("Synced successfully: {cid}"),
                |err| format!("Failed to sync canister: {err}"),
            )
            .await
        });
    }

    // Consume the set of futures and abort if an error occurs
    while let Some(res) = futs.next().await {
        // TODO(or.ricon): Handle canister sync failures
        res?;
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(display("no canisters available to sync"))]
    NoCanisters,

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(transparent)]
    IdLookup { source: LookupError },

    #[snafu(transparent)]
    SyncAdapter { source: AdapterSyncError },
}
