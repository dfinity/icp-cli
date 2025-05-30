use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use serde::Deserialize;
use snafu::Snafu;

/// Configuration for a Motoko-based canister build adapter.
#[derive(Debug, Deserialize)]
pub struct MotokoAdapter {
    /// Optional path to the main Motoko source file.
    /// If omitted, a default like `main.mo` may be assumed.
    #[serde(default)]
    pub main: Option<String>,
}

#[async_trait]
impl Adapter for MotokoAdapter {
    async fn compile(&self, _path: Utf8PathBuf) -> Result<(), AdapterCompileError> {
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum MotokoAdapterCompileError {
    #[snafu(display("an unexpected build error occurred"))]
    Unexpected,
}
