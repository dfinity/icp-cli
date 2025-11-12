use anyhow::anyhow;
use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::{
    agent,
    context::{CanisterSelection, GetAgentForEnvError, GetEnvironmentError},
    identity, network,
};

use icp::context::Context;

use crate::{
    commands::args,
    operations::install::{InstallOperationError, install_canister},
};
use icp::store_id::LookupIdError;

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateAgentError),

    #[error(transparent)]
    LookupCanisterId(#[from] LookupIdError),

    #[error(transparent)]
    GetEnvironment(#[from] GetEnvironmentError),

    #[error(transparent)]
    GetAgentForEnv(#[from] GetAgentForEnvError),

    #[error(transparent)]
    InstallOperation(#[from] InstallOperationError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &InstallArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    let canister = match selections.canister {
        CanisterSelection::Named(name) => name,
        CanisterSelection::Principal(_) => Err(anyhow!("Cannot install canister by principal"))?,
    };

    // Get canister ID
    let canister_id = ctx
        .get_canister_id_for_env(&canister, &selections.environment)
        .await
        .map_err(|e| anyhow!(e))?;

    // Agent
    let agent = ctx
        .get_agent_for_env(&selections.identity, &selections.environment)
        .await?;

    // Lookup the canister build artifact
    let wasm = ctx
        .artifacts
        .lookup(&canister)
        .await
        .map_err(|e| anyhow!(e))?;

    // Management Interface
    let mgmt = ManagementCanister::create(&agent);

    // Install code to the single canister
    install_canister(&mgmt, &canister_id, &canister, &wasm, &args.mode).await?;

    let _ = ctx
        .term
        .write_line(&format!("Canister {canister} installed successfully"));

    Ok(())
}
