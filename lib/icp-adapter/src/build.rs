use crate::{
    motoko::MotokoAdapterCompileError, rust::RustAdapterCompileError,
    script::ScriptAdapterCompileError,
};
use async_trait::async_trait;
use camino::Utf8Path;
use snafu::Snafu;

#[async_trait]
pub trait Adapter {
    async fn compile(
        &self,
        canister_path: &Utf8Path,
        wasm_output_path: &Utf8Path,
    ) -> Result<(), AdapterCompileError>;
}

#[derive(Debug, Snafu)]
pub enum AdapterCompileError {
    #[snafu(transparent)]
    Rust { source: RustAdapterCompileError },

    #[snafu(transparent)]
    Motoko { source: MotokoAdapterCompileError },

    #[snafu(transparent)]
    Script { source: ScriptAdapterCompileError },
}
