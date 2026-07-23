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

use icp_deploy_canister::install_canister_resolved;

use crate::{
    commands::args::{self, ArgsOpt},
    operations::{
        candid_compat::{CandidCompatibility, check_candid_compatibility},
        install::{WasmMemoryPersistenceOpt, is_eop_canister, resolve_install_mode_and_status},
    },
};

/// Install a built WASM to a canister on a network
#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// For Motoko canisters with enhanced orthogonal persistence (EOP), controls whether
    /// the canister's main (Wasm) memory is preserved across an upgrade.
    ///
    /// Only valid with `--mode upgrade` on an EOP canister.
    ///
    /// - `keep`: preserve main memory — the normal EOP upgrade (the default if this flag
    ///   is omitted).
    ///
    /// - `replace`: discard main memory. DANGEROUS: any state not held in `stable`
    ///   variables is lost. Requires interactive confirmation (or `--yes`).
    #[arg(long, value_enum)]
    pub(crate) wasm_memory_persistence: Option<WasmMemoryPersistenceOpt>,

    /// Path to the WASM file to install. Uses the build output if not explicitly provided.
    #[arg(long)]
    pub(crate) wasm: Option<PathBuf>,

    #[command(flatten)]
    pub(crate) args_opt: ArgsOpt,

    /// Skip confirmation prompts, including the Candid interface compatibility check and
    /// the dangerous-operation prompt for `--wasm-memory-persistence replace`.
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

    // Validate --wasm-memory-persistence: only meaningful for upgrades of EOP canisters.
    if let Some(persistence) = args.wasm_memory_persistence {
        if args.mode != "upgrade" {
            bail!(
                "--wasm-memory-persistence can only be used with `--mode upgrade` \
                 (got `--mode {}`). It has no effect for install/reinstall, and `auto` \
                 is ambiguous; pass `--mode upgrade` explicitly.",
                args.mode
            );
        }
        if !is_eop_canister(&agent, &canister_id).await {
            bail!(
                "--wasm-memory-persistence only applies to Motoko canisters with enhanced \
                 orthogonal persistence (EOP). The target canister is not an EOP canister."
            );
        }
        if persistence == WasmMemoryPersistenceOpt::Replace {
            warn!(
                "--wasm-memory-persistence=replace will DISCARD the canister's \
                 main (Wasm) memory."
            );
            warn!(
                "Only state held in `stable` variables survives. Heap state is lost \
                 and cannot be recovered."
            );
            if args.yes {
                info!("Proceeding without confirmation (--yes).");
            } else if std::io::stdin().is_terminal() {
                let confirmed = Confirm::new()
                    .with_prompt("Do you want to proceed?")
                    .default(false)
                    .interact()?;
                if !confirmed {
                    bail!("Operation cancelled by user");
                }
            } else {
                bail!(
                    "Refusing to discard the canister's main memory without confirmation \
                     in a non-interactive context. Use --yes to proceed."
                );
            }
        }
    }

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

    let wmp = args
        .wasm_memory_persistence
        .map(WasmMemoryPersistenceOpt::to_ic);
    install_canister_resolved(
        &canister_display,
        canister_id,
        &wasm,
        install_mode,
        status,
        init_args_bytes.as_deref(),
        wmp,
        &agent,
        args.proxy,
    )
    .await?;

    info!("Canister {canister_display} installed successfully");

    Ok(())
}
