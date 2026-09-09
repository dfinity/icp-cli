use icp_events::StepReporter;
use snafu::prelude::*;

use crate::{canister::wasm, fs, manifest::adapter::prebuilt::Adapter};

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
    wasm: &dyn wasm::Fetch,
) -> Result<(), PrebuiltError> {
    let src = wasm
        .wasm(
            &adapter.source,
            &params.path,
            adapter.sha256.as_deref(),
            reporter,
        )
        .await?;

    reporter.info(format!("Writing WASM file: {}", params.output));
    fs::copy(&src, &params.output).context(CopyFileSnafu)?;

    Ok(())
}
