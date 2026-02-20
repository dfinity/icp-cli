use anyhow::{Context as _, anyhow, bail};
use clap::Args;
use icp::context::{CanisterSelection, Context};
use icp::manifest::InitArgsFormat;
use icp::prelude::*;
use icp::{InitArgs, fs};

use crate::{commands::args, operations::install::install_canister};

/// Install a built WASM to a canister on a network
#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// Path to the WASM file to install. Uses the build output if not explicitly provided.
    #[arg(long)]
    pub(crate) wasm: Option<PathBuf>,

    /// Inline initialization arguments, interpreted per `--args-format` (Candid by default).
    #[arg(long, conflicts_with = "args_file")]
    pub(crate) args: Option<String>,

    /// Path to a file containing initialization arguments.
    #[arg(long, conflicts_with = "args")]
    pub(crate) args_file: Option<PathBuf>,

    /// Format of the initialization arguments.
    #[arg(long, default_value = "candid")]
    pub(crate) args_format: InitArgsFormat,

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

    let init_args = match (&args.args, &args.args_file) {
        (Some(value), None) => {
            if args.args_format == InitArgsFormat::Bin {
                bail!("--args-format bin requires --args-file, not --args");
            }
            Some(InitArgs::Text {
                content: value.clone(),
                format: args.args_format.clone(),
            })
        }
        (None, Some(file_path)) => Some(match args.args_format {
            InitArgsFormat::Bin => {
                let bytes = fs::read(file_path).context("failed to read init args file")?;
                InitArgs::Binary(bytes)
            }
            ref fmt => {
                let content =
                    fs::read_to_string(file_path).context("failed to read init args file")?;
                InitArgs::Text {
                    content: content.trim().to_owned(),
                    format: fmt.clone(),
                }
            }
        }),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
    };

    let init_args_bytes = init_args
        .as_ref()
        .map(|ia| ia.to_bytes().context("failed to encode init args"))
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
