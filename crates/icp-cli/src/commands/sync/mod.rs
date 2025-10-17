use std::collections::HashMap;

use anyhow::anyhow;
use clap::Args;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::{
    agent,
    canister::sync::{Params, SynchronizeError},
    identity, network,
};

use crate::{
    commands::{Context, Mode},
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
    store_id::{Key, LookupError},
};

#[derive(Args, Debug)]
pub(crate) struct SyncArgs {
    /// Canister names
    pub(crate) names: Vec<String>,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
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

    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

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

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            // Load the project
            let p = ctx.project.load().await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

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

            let progress_manager =
                ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });

            // Iterate through each resolved canister and trigger its sync process.
            for (_, (canister_path, c)) in cs {
                // Create progress bar with standard configuration
                let mut pb = progress_manager.create_multi_step_progress_bar(&c.name, "Sync");

                // Get canister principal ID
                let cid = ctx.ids.lookup(&Key {
                    network: env.network.name.to_owned(),
                    environment: env.name.to_owned(),
                    canister: c.name.to_owned(),
                })?;

                // Create an async closure that handles the sync process for this specific canister
                let fut = {
                    let agent = agent.clone();
                    let c = c.clone();

                    async move {
                        // Define the sync logic
                        let sync_result = async {
                            let step_count = c.sync.steps.len();
                            for (i, step) in c.sync.steps.iter().enumerate() {
                                // Indicate to user the current step being executed
                                let current_step = i + 1;
                                let pb_hdr =
                                    format!("\nSyncing: {step} {current_step} of {step_count}");

                                let tx = pb.begin_step(pb_hdr);

                                // Execute step
                                let sync_result = ctx
                                    .syncer
                                    .sync(
                                        step, // step
                                        &Params {
                                            path: canister_path.to_owned(),
                                            cid: cid.to_owned(),
                                        },
                                        &agent,
                                        Some(tx),
                                    )
                                    .await;

                                // Ensure background receiver drains all messages
                                pb.end_step().await;

                                if let Err(e) = sync_result {
                                    return Err(CommandError::Synchronize(e));
                                }
                            }

                            Ok::<_, CommandError>(())
                        }
                        .await;

                        // Execute with progress tracking for final state
                        let result = ProgressManager::execute_with_progress(
                            &pb,
                            async { sync_result },
                            || format!("Synced successfully: {cid}"),
                            |err| format!("Failed to sync canister: {err}"),
                        )
                        .await;

                        // After progress bar is finished, dump the output if sync failed
                        if let Err(e) = &result {
                            pb.dump_output(ctx);
                            let _ = ctx
                                .term
                                .write_line(&format!("Failed to sync canister: {e}"));
                        }

                        result
                    }
                };

                futs.push_back(fut);
            }

            // Consume the set of futures and collect errors
            let mut found_error = false;
            while let Some(res) = futs.next().await {
                if res.is_err() {
                    found_error = true;
                }
            }

            if found_error {
                return Err(CommandError::Synchronize(SynchronizeError::Unexpected(
                    anyhow!("One or more canisters failed to sync"),
                )));
            }
        }
    }

    Ok(())
}
