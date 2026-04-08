use std::io::IsTerminal;

use anyhow::{Context as _, anyhow, bail};
use candid::Principal;
use clap::Args;
use dialoguer::Confirm;
use ic_management_canister_types::CanisterInstallMode;
use icp::context::{CanisterSelection, Context};
use icp::fs;
use icp::prelude::*;
use tracing::{info, warn};

use crate::{
    commands::args::{self, ArgsOpt},
    operations::{
        candid_compat::{CandidCompatibility, check_candid_compatibility},
        install::{install_canister, resolve_install_mode_and_status},
    },
};

/// Install a built WASM to a canister on a network
#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// Path to the WASM file to install. Uses the build output if not explicitly provided.
    #[arg(long)]
    pub(crate) wasm: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) args_opt: ArgsOpt,

    /// Skip confirmation prompts, including the Candid interface compatibility check.
    #[arg(long, short)]
    pub(crate) yes: bool,

    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Principal of a proxy canister to route the management canister call through.
    #[arg(long)]
    pub(crate) proxy: Option<Principal>,
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

    // If you add .did support to this code, consider extracting/unifying with the logic from call.rs
    let init_args_bytes = args.args_opt.resolve_bytes()?;

    let canister_display = args.cmd_args.canister.to_string();
    let (install_mode, status) = resolve_install_mode_and_status(
        &agent,
        args.proxy,
        &canister_display,
        &canister_id,
        &args.mode,
    )
    .await?;

    // Candid interface compatibility check for upgrades
    if !args.yes && matches!(install_mode, CanisterInstallMode::Upgrade(_)) {
        match check_candid_compatibility(&agent, &canister_id, &wasm).await {
            CandidCompatibility::Compatible | CandidCompatibility::Skipped(_) => {}
            CandidCompatibility::Incompatible(details) => {
                let warning = format!(
                    "Candid interface compatibility check failed for canister \
                     '{canister_display}'.\n\
                     You are making a BREAKING change. Other canisters or frontend clients \
                     relying on your canister may stop working.\n\n\
                     {details}"
                );

                if std::io::stdin().is_terminal() {
                    warn!("{warning}");
                    let confirmed = Confirm::new()
                        .with_prompt("Do you want to proceed anyway?")
                        .default(false)
                        .interact()?;
                    if !confirmed {
                        bail!("Installation cancelled.");
                    }
                } else {
                    bail!("{warning}\n\nUse --yes to bypass this check.");
                }
            }
        }
    }

    install_canister(
        &agent,
        args.proxy,
        &canister_id,
        &canister_display,
        &wasm,
        install_mode,
        status,
        init_args_bytes.as_deref(),
    )
    .await?;

    info!("Canister {canister_display} installed successfully");

    Ok(())
}
