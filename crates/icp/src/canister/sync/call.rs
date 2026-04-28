use candid::Encode;
use ic_agent::Agent;
use snafu::prelude::*;

use crate::{
    InitArgs, InitArgsToBytesError, fs,
    manifest::{
        adapter::call::Adapter,
        canister::{ArgsFormat, ManifestInitArgs},
    },
    prelude::*,
};

use super::Params;

#[derive(Debug, Snafu)]
pub enum CallError {
    #[snafu(display("canister '{name}' not found in the current environment"))]
    CanisterNotFound { name: String },

    #[snafu(display("failed to read args file for call to {canister}.{method}"))]
    ReadArgsFile {
        canister: String,
        method: String,
        source: fs::IoError,
    },

    #[snafu(display("cannot use 'bin' format with an inline value for call to {canister}.{method}"))]
    BinFormatInlineArgs { canister: String, method: String },

    #[snafu(display("failed to encode args for call to {canister}.{method}"))]
    EncodeArgs {
        canister: String,
        method: String,
        source: InitArgsToBytesError,
    },

    #[snafu(display("call to {canister}.{method} failed"))]
    Call {
        canister: String,
        method: String,
        source: ic_agent::AgentError,
    },
}

pub(super) async fn sync(adapter: &Adapter, params: &Params, agent: &Agent) -> Result<(), CallError> {
    let cid = params
        .canister_ids
        .get(&adapter.canister)
        .copied()
        .ok_or_else(|| CanisterNotFoundSnafu { name: &adapter.canister }.build())?;

    let arg_bytes = match &adapter.args {
        None => Encode!().expect("empty Candid encoding cannot fail"),
        Some(manifest_args) => resolve_args(manifest_args, &params.path, &adapter.canister, &adapter.method)?
            .to_bytes()
            .context(EncodeArgsSnafu {
                canister: &adapter.canister,
                method: &adapter.method,
            })?,
    };

    agent
        .update(&cid, &adapter.method)
        .with_arg(arg_bytes)
        .call_and_wait()
        .await
        .context(CallSnafu {
            canister: &adapter.canister,
            method: &adapter.method,
        })?;

    Ok(())
}

fn resolve_args(
    manifest_args: &ManifestInitArgs,
    base_path: &Path,
    canister: &str,
    method: &str,
) -> Result<InitArgs, CallError> {
    match manifest_args {
        ManifestInitArgs::String(content) => Ok(InitArgs::Text {
            content: content.trim().to_owned(),
            format: ArgsFormat::Candid,
        }),
        ManifestInitArgs::Path { path, format } => {
            let file_path = base_path.join(path);
            match format {
                ArgsFormat::Bin => {
                    let bytes = fs::read(&file_path).context(ReadArgsFileSnafu { canister, method })?;
                    Ok(InitArgs::Binary(bytes))
                }
                fmt => {
                    let content =
                        fs::read_to_string(&file_path).context(ReadArgsFileSnafu { canister, method })?;
                    Ok(InitArgs::Text {
                        content: content.trim().to_owned(),
                        format: fmt.clone(),
                    })
                }
            }
        }
        ManifestInitArgs::Value { value, format } => match format {
            ArgsFormat::Bin => BinFormatInlineArgsSnafu { canister, method }.fail(),
            fmt => Ok(InitArgs::Text {
                content: value.trim().to_owned(),
                format: fmt.clone(),
            }),
        },
    }
}
