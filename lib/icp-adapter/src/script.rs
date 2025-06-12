use crate::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::Utf8Path;
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};
use std::process::{Command, Stdio};

#[derive(Debug, Deserialize, PartialEq)]
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
#[derive(Debug, Deserialize, PartialEq)]
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

    #[snafu(display("invalid command '{command}': {reason}"))]
    InvalidCommand { command: String, reason: String },

    #[snafu(display("failed to execute command '{command}'"))]
    CommandInvoke {
        command: String,
        source: std::io::Error,
    },

    #[snafu(display("command '{command}' failed with status code {code}"))]
    CommandStatus { command: String, code: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino_tempfile::NamedUtf8TempFile;
    use std::io::Read;

    #[tokio::test]
    async fn single_command() {
        // Create temporary file
        let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

        // Define adapter
        let v = ScriptAdapter {
            command: CommandField::Command(format!("sh -c 'echo test > {}'", f.path())),
        };

        // Invoke adapter
        v.compile("/".into()).await.expect("unexpected failure");

        // Verify command ran
        let mut out = String::new();

        f.read_to_string(&mut out)
            .expect("failed to read temporary file");

        assert_eq!(out, "test\n".to_string());
    }

    #[tokio::test]
    async fn multiple_commands() {
        // Create temporary file
        let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

        // Define adapter
        let v = ScriptAdapter {
            command: CommandField::Commands(vec![
                format!("sh -c 'echo cmd-1 >> {}'", f.path()),
                format!("sh -c 'echo cmd-2 >> {}'", f.path()),
                format!("sh -c 'echo cmd-3 >> {}'", f.path()),
            ]),
        };

        // Invoke adapter
        v.compile("/".into()).await.expect("unexpected failure");

        // Verify command ran
        let mut out = String::new();

        f.read_to_string(&mut out)
            .expect("failed to read temporary file");

        assert_eq!(out, "cmd-1\ncmd-2\ncmd-3\n".to_string());
    }

    #[tokio::test]
    async fn invalid_command() {
        // Define adapter
        let v = ScriptAdapter {
            command: CommandField::Command("".into()),
        };

        // Invoke adapter
        let out = v.compile("/".into()).await;

        // Assert failure
        assert!(matches!(
            out,
            Err(AdapterCompileError::Script {
                source: ScriptAdapterCompileError::InvalidCommand { .. }
            })
        ));
    }

    #[tokio::test]
    async fn failed_command_not_found() {
        // Define adapter
        let v = ScriptAdapter {
            command: CommandField::Command("invalid-command".into()),
        };

        // Invoke adapter
        let out = v.compile("/".into()).await;

        println!("{out:?}");

        // Assert failure
        assert!(matches!(
            out,
            Err(AdapterCompileError::Script {
                source: ScriptAdapterCompileError::CommandInvoke { .. }
            })
        ));
    }

    #[tokio::test]
    async fn failed_command_error_status() {
        // Define adapter
        let v = ScriptAdapter {
            command: CommandField::Command("sh -c 'exit 1'".into()),
        };

        // Invoke adapter
        let out = v.compile("/".into()).await;

        println!("{out:?}");

        // Assert failure
        assert!(matches!(
            out,
            Err(AdapterCompileError::Script {
                source: ScriptAdapterCompileError::CommandStatus { .. }
            })
        ));
    }
}
