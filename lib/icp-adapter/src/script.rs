use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use serde::Deserialize;
use snafu::Snafu;

/// Configuration for a custom canister build adapter.
#[derive(Debug, Deserialize)]
pub struct ScriptAdapter {
    /// Path to a script or executable used to build the canister.
    pub source: String,
}

#[async_trait]
impl Adapter for ScriptAdapter {
    async fn compile(&self, _path: Utf8PathBuf) -> Result<(), AdapterCompileError> {
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum ScriptAdapterCompileError {
    #[snafu(display("an unexpected build error occurred"))]
    Unexpected,
}
