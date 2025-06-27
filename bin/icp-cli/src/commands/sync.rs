use crate::env::Env;
use clap::Parser;
use icp_adapter::sync::{Adapter, AdapterSyncError};
use icp_canister::model::AdapterSync;
use icp_project::{
    directory::{FindProjectError, ProjectDirectory},
    model::{LoadProjectManifestError, ProjectManifest},
};
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: Option<String>,
}

pub async fn exec(_env: &Env, cmd: Cmd) -> Result<(), CommandError> {
    // Find the current ICP project directory.
    let pd = ProjectDirectory::find()?.ok_or(CommandError::ProjectNotFound)?;

    // Get the project directory structure for path resolution.
    let pds = pd.structure();

    // Load the project manifest, which defines the canisters to be synced.
    let pm = ProjectManifest::load(pds)?;

    // Choose canisters to sync
    let canisters = pm
        .canisters
        .into_iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Check if selected canister exists
    if let Some(name) = cmd.name {
        if canisters.is_empty() {
            return Err(CommandError::CanisterNotFound { name });
        }
    }

    // Verify at least one canister is available to create
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
    FindProjectError { source: FindProjectError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectLoad { source: LoadProjectManifestError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to install"))]
    NoCanisters,

    #[snafu(transparent)]
    SyncAdapter { source: AdapterSyncError },
}
