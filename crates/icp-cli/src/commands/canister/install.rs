use anyhow::{Context as _, anyhow, bail};
use clap::Args;
use dialoguer::Confirm;
use icp::context::{CanisterSelection, Context};
use icp::fs;
use icp::prelude::*;

use crate::{
    commands::args,
    operations::{
        install::{WasmMemoryPersistenceOpt, install_canister, is_eop_canister},
        misc::{ParsedArguments, parse_args},
    },
};

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

    /// Skip the interactive confirmation prompt for dangerous operations
    /// (currently: `--wasm-memory-persistence replace`).
    #[arg(long, short = 'y')]
    pub(crate) yes: bool,

    /// Path to the WASM file to install. Uses the build output if not explicitly provided.
    #[arg(long)]
    pub(crate) wasm: Option<PathBuf>,

    /// Initialization arguments for the canister.
    /// Can be:
    ///
    /// - Hex-encoded bytes (e.g., `4449444c00`)
    ///
    /// - Candid text format (e.g., `(42)` or `(record { name = "Alice" })`)
    ///
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
            ctx.term.write_line(
                "Warning: --wasm-memory-persistence=replace will DISCARD the canister's \
                 main (Wasm) memory.",
            )?;
            ctx.term.write_line(
                "Only state held in `stable` variables survives. Heap state is lost \
                 and cannot be recovered.",
            )?;
            if args.yes {
                ctx.term
                    .write_line("Proceeding without confirmation (--yes).")?;
            } else {
                let confirmed = Confirm::new()
                    .with_prompt("Do you want to proceed?")
                    .default(false)
                    .interact()?;
                if !confirmed {
                    bail!("Operation cancelled by user");
                }
            }
        }
    }

    let canister_display = args.cmd_args.canister.to_string();
    install_canister(
        &agent,
        &canister_id,
        &canister_display,
        &wasm,
        &args.mode,
        init_args_bytes.as_deref(),
        args.wasm_memory_persistence,
    )
    .await?;

    let _ = ctx.term.write_line(&format!(
        "Canister {canister_display} installed successfully"
    ));

    Ok(())
}
