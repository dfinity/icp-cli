use crate::commands::canister::install::CanisterInstallError::GetProject;
use crate::env::GetProjectError;
use crate::{
    env::Env, store_artifact::LookupError as LookupArtifactError,
    store_id::LookupError as LookupIdError,
};
use clap::Parser;
use ic_agent::{Agent, AgentError};
use ic_utils::interfaces::management_canister::builders::InstallMode;
use icp_identity::key::LoadIdentityInContextError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterInstallCmd {
    /// The name of the canister within the current project
    pub name: Option<String>,

    /// The URL of the IC network endpoint
    #[clap(long, default_value = "http://127.0.0.1:8000")]
    pub network_url: String,
}

pub async fn exec(env: &Env, cmd: CanisterInstallCmd) -> Result<(), CanisterInstallError> {
    let pm = env.project()?;

    // Load the currently selected identity
    let identity = env.load_identity()?;

    // Create an agent pointing to the desired network endpoint
    let agent = Agent::builder()
        .with_url(&cmd.network_url)
        .with_arc_identity(identity)
        .build()?;

    // TODO(or.ricon): This is to be replaced with a centralized agent or agent-builder
    if cmd.network_url.contains("127.0.0.1") || cmd.network_url.contains("localhost") {
        agent.fetch_root_key().await?;
    }

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Choose canisters to install
    let canisters = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Case 1 (canister not found)
    if let Some(name) = cmd.name {
        if canisters.is_empty() {
            return Err(CanisterInstallError::CanisterNotFound { name });
        }
    }

    // Case 2 (no canisters)
    if canisters.is_empty() {
        return Err(CanisterInstallError::NoCanisters);
    }

    for (_, c) in canisters {
        // Lookup the canister id
        let cid = env.id_store.lookup(&c.name)?;

        // Lookup the canister build artifact
        let wasm = env.artifact_store.lookup(&c.name)?;

        mgmt.install_code(&cid, &wasm)
            .with_mode(InstallMode::Install)
            .await?;

        eprintln!(
            "Installed WASM payload to canister '{}' (ID: '{}')",
            c.name, cid,
        );
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterInstallError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("no canisters available to install"))]
    NoCanisters,

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    LookupCanisterArtifact { source: LookupArtifactError },

    #[snafu(transparent)]
    InstallAgent { source: AgentError },
}
