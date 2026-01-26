use anyhow::{Context as _, anyhow};
use clap::Args;
use icp::context::{CanisterSelection, Context};
use icp::fs;
use icp::prelude::*;

use crate::{
    commands::args,
    operations::{
        install::install_canister,
        misc::{ParsedArguments, parse_args},
    },
};

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// Path to the WASM file to install. Uses the build output if not explicitly provided.
    #[arg(long)]
    pub(crate) wasm: Option<PathBuf>,

    /// Initialization arguments for the canister.
    /// Can be:
    /// - Hex-encoded bytes (e.g., `4449444c00`)
    /// - Candid text format (e.g., `(42)` or `(record { name = "Alice" })`)
    /// - File path (e.g., `args.txt` or `./path/to/args.candid`)
    ///   The file should contain either hex or Candid format arguments.
    #[arg(long)]
    pub(crate) args: Option<String>,

    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &InstallArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    let wasm = if let Some(wasm_path) = &args.wasm {
        // Read from file
        fs::read(wasm_path).context("failed to read WASM file")?
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
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let canister_id = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Parse init_args if provided, resolving file paths relative to current working directory (CLI input)
    let init_args_bytes = args
        .args
        .as_ref()
        .map(|s| {
            let cwd =
                dunce::canonicalize(".").context("Failed to get current working directory")?;
            let cwd =
                PathBuf::try_from(cwd).context("Current directory path is not valid UTF-8")?;
            match parse_args(s, &cwd)? {
                ParsedArguments::Hex(bytes) => Ok(bytes),
                ParsedArguments::Candid(args) => args
                    .to_bytes()
                    .context("Failed to encode Candid args to bytes"),
            }
        })
        .transpose()?;

    let canister_display = args.cmd_args.canister.to_string();
    install_canister(
        &agent,
        &canister_id,
        &canister_display,
        &wasm,
        &args.mode,
        init_args_bytes.as_deref(),
    )
    .await?;

    let _ = ctx.term.write_line(&format!(
        "Canister {canister_display} installed successfully"
    ));

    Ok(())
}
