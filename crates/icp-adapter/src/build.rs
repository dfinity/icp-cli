use async_trait::async_trait;
use icp::prelude::*;
use snafu::Snafu;

use crate::{pre_built::PrebuiltAdapterCompileError, script::ScriptAdapterCompileError};

#[async_trait]
pub trait Adapter {
    async fn compile(
        &self,
        canister_path: &Path,
        wasm_output_path: &Path,
    ) -> Result<String, AdapterCompileError>;
}

#[derive(Debug, Snafu)]
pub enum AdapterCompileError {
    #[snafu(transparent)]
    Script { source: ScriptAdapterCompileError },

    #[snafu(transparent)]
    Prebuilt { source: PrebuiltAdapterCompileError },
}
