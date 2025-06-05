use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};
use std::process::{Command, Stdio};

/// Configuration for a custom canister build adapter.
#[derive(Debug, Deserialize)]
pub struct ScriptAdapter {
    /// Command used to build a canister
    pub command: String,
}

#[async_trait]
impl Adapter for ScriptAdapter {
    async fn compile(&self, path: Utf8PathBuf) -> Result<(), AdapterCompileError> {
        // Parse command input
        let input = shellwords::split(&self.command).context(CommandParseSnafu {
            command: &self.command,
        })?;

        // Separate command and args
        let (cmd, args) = input.split_first().context(InvalidCommandSnafu {
            command: &self.command,
            reason: "command must include at least one element".to_string(),
        })?;

        // Command
        let mut cmd = Command::new(cmd);

        // Args
        cmd.args(args);

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
    #[snafu(display("failed to parse command {command}: {source}"))]
    CommandParse {
        command: String,
        source: shellwords::MismatchedQuotes,
    },

    #[snafu(display("invalid command {command}: {reason}"))]
    InvalidCommand { command: String, reason: String },

    #[snafu(display("failed to execute command {command}: {source}"))]
    CommandInvoke {
        command: String,
        source: std::io::Error,
    },

    #[snafu(display("command {command} failed with status code {code}"))]
    CommandStatus { command: String, code: String },
}
