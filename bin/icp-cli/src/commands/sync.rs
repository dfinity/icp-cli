use std::{collections::HashSet, time::Duration};

use crate::{
    COLOR_FAILURE, COLOR_REGULAR, COLOR_SUCCESS, RollingLines, TICK_EMPTY, TICK_FAILURE,
    TICK_SUCCESS,
    context::{Context, ContextGetAgentError, GetProjectError},
    make_style,
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError},
};
use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use icp_adapter::sync::{Adapter, AdapterSyncError};
use icp_canister::SyncStep;
use indicatif::{MultiProgress, ProgressBar};
use itertools::Itertools;
use snafu::Snafu;
use tokio::sync::mpsc;
use tracing::debug;

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The names of the canisters within the current project
    pub names: Vec<String>,

    #[clap(flatten)]
    pub identity: IdentityOpt,

    #[clap(flatten)]
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

    let mp = MultiProgress::new();

    // Iterate through each resolved canister and trigger its sync process.
    for (canister_path, c) in cs {
        // Attach spinner to multi-progress-bar container
        let pb = mp.add(ProgressBar::new_spinner().with_style(make_style(
            TICK_EMPTY,    // end_tick
            COLOR_REGULAR, // color
        )));

        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        // Set the progress bar prefix to display the canister name in brackets
        pb.set_prefix(format!("[{}]", c.name));

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

                    // Shared progress-bar messaging utility
                    let set_message = {
                        let pb = pb.clone();
                        let pb_hdr = pb_hdr.clone();

                        move |msg: String| {
                            pb.set_message(format!("{pb_hdr}\n\n{msg}\n"));
                        }
                    };

                    pb.set_message(pb_hdr);

                    match step {
                        // Synchronize the canister using the custom script adapter.
                        SyncStep::Script(adapter) => {
                            // Create a channel for the script adapter to pass terminal output to
                            let (tx, mut rx) = mpsc::channel::<String>(100);

                            // Create a rolling buffer to contain last N lines of terminal output
                            let mut lines = RollingLines::new(4);

                            // Handle logging from script commands
                            tokio::spawn(async move {
                                while let Some(line) = rx.recv().await {
                                    debug!(line);

                                    // Update output buffer
                                    lines.push(line);

                                    // Update progress-bar with rolling terminal output
                                    let msg = lines.iter().join("\n");
                                    set_message(msg);
                                }
                            });

                            adapter
                                .with_stdio_sender(tx)
                                .sync(canister_path, &cid, agent)
                                .await?
                        }

                        // Synchronize the canister using the assets adapter.
                        SyncStep::Assets(adapter) => {
                            adapter.sync(canister_path, &cid, agent).await?
                        }
                    };
                }

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the sync function and capture the result
            let out = sync_fn.await;

            // Update the progress bar style based on result
            pb.set_style(match &out {
                Ok(_) => make_style(TICK_SUCCESS, COLOR_SUCCESS),
                Err(_) => make_style(TICK_FAILURE, COLOR_FAILURE),
            });

            // Update the progress bar message based on result
            // TODO: Add instructions for accessing the canister
            pb.set_message(match &out {
                Ok(_) => format!("Synced successfully: {cid}"),
                Err(err) => format!("Failed to sync canister: {err}"),
            });

            // Stop the progress bar spinner and keep the final state visible
            pb.finish();

            out
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
    GetProject { source: GetProjectError },

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
    GetAgent { source: ContextGetAgentError },

    #[snafu(transparent)]
    IdLookup { source: LookupError },

    #[snafu(transparent)]
    SyncAdapter { source: AdapterSyncError },
}
