use std::collections::HashMap;

use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::{
    agent,
    canister::sync::{Params, SynchronizeError},
    identity, network,
};

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ScriptProgressHandler},
    store_id::{Key, LookupError},
};

use super::ContextError;

#[derive(Parser, Debug)]
pub struct Cmd {
    /// Canister names
    pub names: Vec<String>,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    EnvironmentNotFound(#[from] ContextError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error("no canisters available to sync")]
    NoCanisters,

    #[error(transparent)]
    IdLookup(#[from] LookupError),

    #[error(transparent)]
    Synchronize(#[from] SynchronizeError),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the environment
    let env = ctx.get_environment(cmd.environment.name()).await?;

    // Agent
    let agent = ctx.get_agent(&env, cmd.identity.into()).await?;

    // The list of canisters we want to sync
    let cnames = match cmd.names.is_empty() {
        // No canisters specified
        true => env.canisters.keys().cloned().collect(),

        // Inividual canisters specified
        false => {
            // Check that the args are valid
            for name in &cmd.names {
                if !env.canisters.contains_key(name) {
                    return Err(CommandError::EnvironmentCanister {
                        environment: env.name.to_owned(),
                        canister: name.to_owned(),
                    });
                }
            }

            cmd.names
        }
    };

    let cs = env
        .canisters
        .iter()
        .filter(|(k, _)| cnames.contains(k))
        .collect::<HashMap<_, _>>();

    // Verify at least one canister is selected to sync
    if cs.is_empty() {
        return Err(CommandError::NoCanisters);
    }

    // Prepare a futures set for concurrent canister syncs
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    // Iterate through each resolved canister and trigger its sync process.
    for (_, (canister_path, c)) in cs {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Get canister principal ID
        let cid = ctx.ids.lookup(&Key {
            network: env.network.name.to_owned(),
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        })?;

        // Create an async closure that handles the sync process for this specific canister
        let sync_fn = {
            let pb = pb.clone();
            let agent = agent.clone();

            async move {
                for step in &c.sync.steps {
                    // Indicate to user the current step being executed
                    let pb_hdr = format!("Syncing: {step}");

                    let script_handler = ScriptProgressHandler::new(pb.clone(), pb_hdr.clone());

                    // Setup script progress handling and receiver join handle
                    let (tx, rx) = script_handler.setup_output_handler();

                    // Execute step
                    ctx.syncer
                        .sync(
                            step, // step
                            &Params {
                                path: canister_path.to_owned(),
                                cid: cid.to_owned(),
                            },
                            &agent,
                            Some(tx),
                        )
                        .await?;

                    // Ensure background receiver drains all messages
                    let _ = rx.await;
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
