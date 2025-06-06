use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};
use std::process::{Command, Stdio};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandField {
    /// Command used to build a canister
    Command(String),

    /// Set of commands used to build a canister
    Commands(Vec<String>),
}

/// Configuration for a custom canister build adapter.
#[derive(Debug, Deserialize)]
pub struct ScriptAdapter {
    /// Command used to build a canister
    #[serde(flatten)]
    pub command: CommandField,
}

#[async_trait]
impl Adapter for ScriptAdapter {
    async fn compile(&self, path: Utf8PathBuf) -> Result<(), AdapterCompileError> {
        let cmds = match &self.command {
            CommandField::Command(cmd) => std::slice::from_ref(cmd),
            CommandField::Commands(cmds) => cmds,
        };

        for input_cmd in cmds {
            // Parse command input
            let input =
                shellwords::split(input_cmd).context(CommandParseSnafu { command: input_cmd })?;

            // Separate command and args
            let (cmd, args) = input.split_first().context(InvalidCommandSnafu {
                command: input_cmd,
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
            let status = cmd
                .status()
                .context(CommandInvokeSnafu { command: input_cmd })?;

            // Status
            if !status.success() {
                return Err(ScriptAdapterCompileError::CommandStatus {
                    command: input_cmd.to_owned(),
                    code: status.code().map_or("N/A".to_string(), |c| c.to_string()),
                }
                .into());
            }
        }

        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum ScriptAdapterCompileError {
    #[snafu(display("failed to parse command '{command}'"))]
    CommandParse {
        command: String,
        source: shellwords::MismatchedQuotes,
    },

    #[snafu(display("invalid command '{command}'"))]
    InvalidCommand { command: String, reason: String },

    #[snafu(display("failed to execute command '{command}'"))]
    CommandInvoke {
        command: String,
        source: std::io::Error,
    },

    #[snafu(display("command '{command}' failed with status code {code}"))]
    CommandStatus { command: String, code: String },
}
