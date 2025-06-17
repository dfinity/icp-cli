use crate::env::Env;
use clap::Parser;
use ic_agent::{Agent, AgentError, export::Principal};
use ic_utils::interfaces::management_canister::builders::InstallMode;
use icp_identity::key::LoadIdentityInContextError;
use icp_project::{
    directory::{FindProjectError, ProjectDirectory},
    model::{LoadProjectManifestError, ProjectManifest},
};
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterInstallCmd {
    /// The name of the canister within the current project
    name: Option<String>,

    /// The URL of the IC network endpoint
    #[clap(long, default_value = "http://127.0.0.1:8000")]
    network_url: String,
}

pub async fn exec(env: &Env, cmd: CanisterInstallCmd) -> Result<(), CanisterInstallError> {
    // Find the current ICP project directory.
    let pd = ProjectDirectory::find()?.ok_or(CanisterInstallError::ProjectNotFound)?;

    // Get the project directory structure for path resolution.
    let pds = pd.structure();

    // Load the project manifest, which defines the canisters to be built.
    let pm = ProjectManifest::load(pds)?;

    // Load the currently selected identity
    let identity = env.load_identity()?;

    // Create an agent pointing to the desired network endpoint
    let agent = Agent::builder()
        .with_url(&cmd.network_url)
        .with_arc_identity(identity)
        .build()?;

    if cmd.network_url.contains("127.0.0.1") || cmd.network_url.contains("localhost") {
        agent.fetch_root_key().await?;
    }

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Choose canisters to install
    let cs = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Case 1 (canister not found)
    if let Some(name) = cmd.name {
        if cs.is_empty() {
            return Err(CanisterInstallError::CanisterNotFound { name });
        }
    }

    // Case 2 (no canisters)
    if cs.is_empty() {
        return Err(CanisterInstallError::NoCanisters);
    }

    for (_, c) in cs {
        let cid = Principal::anonymous();
        let wasm = vec![];

        mgmt.install_code(&cid, &wasm)
            .with_mode(InstallMode::Install)
            .await?;

        println!(
            "Installed WASM payload to canister '{}' (ID: '{}')",
            c.name, cid,
        );
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterInstallError {
    #[snafu(transparent)]
    FindProjectError { source: FindProjectError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectLoad { source: LoadProjectManifestError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to build"))]
    NoCanisters,

    #[snafu(transparent)]
    InstallAgent { source: AgentError },
}
