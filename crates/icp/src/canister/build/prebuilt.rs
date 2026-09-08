use icp_events::StepReporter;
use snafu::prelude::*;

use crate::{
    canister::{ReporterProgress, wasm},
    fs,
    manifest::adapter::prebuilt::Adapter,
    package::PackageCache,
};

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
    reporter: &StepReporter,
    pkg_cache: &PackageCache,
) -> Result<(), PrebuiltError> {
    let src = wasm::resolve(
        &adapter.source,
        &params.path,
        adapter.sha256.as_deref(),
        Some(&ReporterProgress(reporter)),
        pkg_cache,
    )
    .await?;

    reporter.info(format!("Writing WASM file: {}", params.output));
    fs::copy(&src, &params.output).context(CopyFileSnafu)?;

    Ok(())
}
