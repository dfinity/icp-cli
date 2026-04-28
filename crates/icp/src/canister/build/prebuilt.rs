use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::{
    canister::wasm, fs::write, manifest::adapter::prebuilt::Adapter, package::PackageCache,
};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PrebuiltError {
    #[snafu(display("failed to send log message"))]
    Log {
        source: tokio::sync::mpsc::error::SendError<String>,
    },

    #[snafu(transparent)]
    Wasm { source: wasm::WasmError },

    #[snafu(display("failed to write wasm output file"))]
    WriteFile { source: crate::fs::IoError },
}

pub(super) async fn build(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<Sender<String>>,
    pkg_cache: &PackageCache,
) -> Result<(), PrebuiltError> {
    let wasm_bytes = wasm::resolve(
        &adapter.source,
        &params.path,
        adapter.sha256.as_deref(),
        stdio.as_ref(),
        pkg_cache,
    )
    .await?;

    if let Some(tx) = &stdio {
        tx.send(format!("Writing WASM file: {}", params.output))
            .await
            .context(LogSnafu)?;
    }
    write(&params.output, &wasm_bytes).context(WriteFileSnafu)?;

    Ok(())
}
