use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8Path;
use serde::Deserialize;
use snafu::Snafu;

/// Configuration for a Rust-based canister build adapter.
#[derive(Debug, Deserialize, PartialEq)]
pub struct RustAdapter {
    /// The name of the Cargo package to build.
    pub package: String,
}

#[async_trait]
impl Adapter for RustAdapter {
    async fn compile(
        &self,
        _canister_path: &Utf8Path,
        _wasm_output_path: &Utf8Path,
    ) -> Result<(), AdapterCompileError> {
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum RustAdapterCompileError {
    #[snafu(display("an unexpected build error occurred"))]
    Unexpected,
}
