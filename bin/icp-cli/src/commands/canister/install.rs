use crate::env::{EnvGetAgentError, GetProjectError};
use crate::options::NetworkOpt;
use crate::{
    env::Env, store_artifact::LookupError as LookupArtifactError,
    store_id::LookupError as LookupIdError,
};
use clap::Parser;
use ic_agent::AgentError;
use ic_utils::interfaces::management_canister::builders::InstallMode;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterInstallCmd {
    /// The name of the canister within the current project
    pub name: Option<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub mode: String,

    #[clap(flatten)]
    pub network: NetworkOpt,
}

pub async fn exec(env: &Env, cmd: CanisterInstallCmd) -> Result<(), CanisterInstallError> {
    env.set_network_opt(cmd.network);

    let pm = env.project()?;

    let agent = env.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

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
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    EnvGetAgent { source: EnvGetAgentError },

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
