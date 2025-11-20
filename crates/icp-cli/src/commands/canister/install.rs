use anyhow::anyhow;
use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::fs;
use icp::{context::CanisterSelection, prelude::*};

use icp::context::{Context, GetAgentError, GetCanisterIdError};

use crate::{
    commands::args,
    operations::install::{InstallOperationError, install_canister},
};

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// Path to the WASM file to install. Uses the build output if not explicitly provided.
    #[arg(long)]
    pub(crate) wasm: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    InstallOperation(#[from] InstallOperationError),

    #[error("failed to read WASM file: {0}")]
    ReadWasmFile(#[from] fs::Error),

    #[error(transparent)]
    GetAgent(#[from] GetAgentError),

    #[error(transparent)]
    GetCanisterId(#[from] GetCanisterIdError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &InstallArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();

    let wasm = if let Some(wasm_path) = &args.wasm {
        // Read from file
        fs::read(wasm_path)?
    } else {
        // Use artifact store (requires named canister)
        let canister = match &selections.canister {
            CanisterSelection::Named(name) => name,
            CanisterSelection::Principal(_) => {
                return Err(anyhow!(
                    "Cannot install canister by principal without --wasm flag"
                ))?;
            }
        };
        ctx.artifacts
            .lookup(canister)
            .await
            .map_err(|e| anyhow!(e))?
    };

    let agent = ctx
        .get_agent(
            &selections.environment,
            &selections.network,
            &selections.identity,
        )
        .await?;
    let canister_id = ctx
        .get_canister_id(
            &selections.canister,
            &selections.environment,
            &selections.network,
        )
        .await?;

    let mgmt = ManagementCanister::create(&agent);
    let canister_display = args.cmd_args.canister.to_string();
    install_canister(&mgmt, &canister_id, &canister_display, &wasm, &args.mode).await?;

    let _ = ctx.term.write_line(&format!(
        "Canister {canister_display} installed successfully"
    ));

    Ok(())
}
