use std::process::{Command, Stdio};

use crate::{Adapter, AdapterCompileError, shell::SHELL};
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
    async fn compile(&self, path: Utf8PathBuf) -> Result<(), AdapterCompileError> {
        // Command
        let mut cmd = Command::new(SHELL.binary());

        // Script
        cmd.arg(SHELL.exec_flag()).arg(&self.source);

        // Set directory
        cmd.current_dir(&path);

        // Output
        // cmd.stdout(Stdio::)

        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum ScriptAdapterCompileError {
    #[snafu(display("an unexpected build error occurred"))]
    Unexpected,
}
