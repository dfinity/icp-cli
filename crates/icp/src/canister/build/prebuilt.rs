use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::{canister::wasm, fs, manifest::adapter::prebuilt::Adapter, package::PackageCache};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PrebuiltError {
    #[snafu(transparent)]
    Wasm { source: wasm::WasmError },

    #[snafu(display("failed to copy wasm to output file"))]
    CopyFile { source: crate::fs::CopyError },
}

pub(super) async fn build(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<Sender<String>>,
    pkg_cache: &PackageCache,
) -> Result<(), PrebuiltError> {
    let src = wasm::resolve(
        &adapter.source,
        &params.path,
        adapter.sha256.as_deref(),
        stdio.as_ref(),
        pkg_cache,
    )
    .await?;

    if let Some(tx) = &stdio {
        let _ = tx
            .send(format!("Writing WASM file: {}", params.output))
            .await;
    }
    fs::copy(&src, &params.output).context(CopyFileSnafu)?;

    Ok(())
}
