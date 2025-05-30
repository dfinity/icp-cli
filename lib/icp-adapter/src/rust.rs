use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use serde::Deserialize;
use snafu::Snafu;

/// Configuration for a Rust-based canister build adapter.
#[derive(Debug, Deserialize)]
pub struct RustAdapter {
    /// The name of the Cargo package to build.
    pub package: String,
}

#[async_trait]
impl Adapter for RustAdapter {
    async fn compile(&self, _path: Utf8PathBuf) -> Result<(), AdapterCompileError> {
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum RustAdapterCompileError {
    #[snafu(display("an unexpected build error occurred"))]
    Unexpected,
}
