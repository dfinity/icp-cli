use std::process::{Command, Stdio};

use crate::{Adapter, AdapterCompileError, shell::SHELL};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use serde::Deserialize;
use snafu::{ResultExt, Snafu};

/// Configuration for a custom canister build adapter.
#[derive(Debug, Deserialize)]
pub struct ScriptAdapter {
    /// Command used to build a canister
    pub command: String,
}

#[async_trait]
impl Adapter for ScriptAdapter {
    async fn compile(&self, path: Utf8PathBuf) -> Result<(), AdapterCompileError> {
        // Command
        let mut cmd = Command::new(SHELL.binary());

        // Script
        cmd.arg(SHELL.exec_flag()).arg(&self.command);

        // Set directory
        cmd.current_dir(&path);

        // Output
        cmd.stdin(Stdio::inherit());
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        // Execute
        let status = cmd.status().context(CommandInvokeSnafu {
            command: &self.command,
        })?;

        // Status
        if !status.success() {
            return Err(ScriptAdapterCompileError::CommandStatus {
                command: self.command.to_owned(),
                code: status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or("N/A".to_string()),
            }
            .into());
        }

        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum ScriptAdapterCompileError {
    #[snafu(display("failed to execute command {command}: {source}"))]
    CommandInvoke {
        command: String,
        source: std::io::Error,
    },

    #[snafu(display("command {command} failed with status code {code}"))]
    CommandStatus { command: String, code: String },
}
