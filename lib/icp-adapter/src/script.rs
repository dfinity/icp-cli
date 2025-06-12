use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8Path;
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

impl CommandField {
    fn as_vec(&self) -> Vec<String> {
        match self {
            Self::Command(cmd) => vec![cmd.clone()],
            Self::Commands(cmds) => cmds.clone(),
        }
    }
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
    async fn compile(&self, path: &Utf8Path) -> Result<(), AdapterCompileError> {
        for input_cmd in self.command.as_vec() {
            // Parse command input
            let input = shellwords::split(&input_cmd).context(CommandParseSnafu {
                command: &input_cmd,
            })?;

            // Separate command and args
            let (cmd, args) = input.split_first().context(InvalidCommandSnafu {
                command: &input_cmd,
                reason: "command must include at least one element".to_string(),
            })?;

            // Try resolving the command as a local path (e.g., ./mytool)
            let cmd = match dunce::canonicalize(path.join(cmd)) {
                // Use the canonicalized local path if it exists
                Ok(p) => p,

                // Fall back to assuming it's a command in the system PATH
                Err(_) => cmd.into(),
            };

            // Command
            let mut cmd = Command::new(cmd);

            // Args
            cmd.args(args);

            // Set directory
            cmd.current_dir(path);

            // Output
            cmd.stdin(Stdio::inherit());
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());

            // Execute
            let status = cmd.status().context(CommandInvokeSnafu {
                command: &input_cmd,
            })?;

            // Status
            if !status.success() {
                return Err(ScriptAdapterCompileError::CommandStatus {
                    command: input_cmd,
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
