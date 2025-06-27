use crate::{
    env::Env, store_artifact::LookupError as LookupArtifactError,
    store_id::LookupError as LookupIdError,
};
use clap::Parser;
use ic_agent::{Agent, AgentError};
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
    pub name: Option<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub mode: String,

    /// The URL of the IC network endpoint
    #[clap(long, default_value = "http://127.0.0.1:8000")]
    pub network_url: String,
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

    // Ensure at least one canister has been selected
    if canisters.is_empty() {
        return Err(match cmd.name {
            // Selected canister not found
            Some(name) => CanisterInstallError::CanisterNotFound { name },

            // No canisters found at all
            None => CanisterInstallError::NoCanisters,
        });
    }

    for (_, c) in canisters {
        // Lookup the canister id
        let cid = env.id_store.lookup(&c.name)?;

        // Lookup the canister build artifact
        let wasm = env.artifact_store.lookup(&c.name)?;

        // Retrieve canister status
        let (status,) = mgmt.canister_status(&cid).await?;

        let install_mode = match cmd.mode.as_ref() {
            // Auto
            "auto" => match status.module_hash {
                // Canister has had code installed to it.
                Some(_) => InstallMode::Upgrade(None),

                // Canister has not had code installed to it.
                None => InstallMode::Install,
            },

            // Install
            "install" => InstallMode::Install,

            // Reinstall
            "reinstall" => InstallMode::Reinstall,

            // Upgrade
            "upgrade" => InstallMode::Upgrade(None),

            // invalid
            _ => panic!("invalid install mode"),
        };

        // Install code to canister
        mgmt.install_code(&cid, &wasm)
            .with_mode(install_mode)
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
    FindProjectError { source: FindProjectError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectLoad { source: LoadProjectManifestError },

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
