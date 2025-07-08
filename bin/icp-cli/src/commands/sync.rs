use crate::env::{Env, GetProjectError};
use crate::options::IdentityOpt;
use clap::Parser;
use icp_adapter::sync::{Adapter, AdapterSyncError};
use icp_canister::model::AdapterSync;
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: Option<String>,

    #[clap(flatten)]
    pub identity: IdentityOpt,
}

pub async fn exec(env: &Env, cmd: Cmd) -> Result<(), CommandError> {
    env.require_identity(cmd.identity.name());

    // Load the project manifest, which defines the canisters to be synced.
    let pm = env.project()?;

    // Choose canisters to sync
    let canisters = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .cloned()
        .collect::<Vec<_>>();

    // Check if selected canister exists
    if let Some(name) = cmd.name {
        if canisters.is_empty() {
            return Err(CommandError::CanisterNotFound { name });
        }
    }

    // Verify at least one canister is available to sync
    if canisters.is_empty() {
        return Err(CommandError::NoCanisters);
    }

    // Iterate through each resolved canister and trigger its sync process.
    for (canister_path, c) in canisters {
        for step in c.sync.into_vec() {
            match step.adapter {
                // Synchronize the canister using the custom script adapter.
                AdapterSync::Script(adapter) => adapter.sync(&canister_path).await?,
            };
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to sync"))]
    NoCanisters,

    #[snafu(transparent)]
    SyncAdapter { source: AdapterSyncError },
}
