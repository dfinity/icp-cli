use std::process::Stdio;

use anyhow::Context;
use async_trait::async_trait;
use ic_agent::Agent;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    join,
    process::Command,
    sync::mpsc::Sender,
};

use crate::canister::{
    build::{self, Build, BuildError},
    sync::{self, Synchronize, SynchronizeError},
};

pub struct Script;

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("failed to parse command: '{command}'")]
    Parse { command: String },

    #[error("invalid command '{command}': {reason}")]
    InvalidCommand { command: String, reason: String },

    #[error("failed to execute command '{command}'")]
    Invoke { command: String },

    #[error("command '{command}' failed with status code {code}")]
    Status { command: String, code: String },
}

#[async_trait]
impl Build for Script {
    async fn build(
        &self,
        step: &build::Step,
        params: &build::Params,
        stdio: Sender<String>,
    ) -> Result<(), BuildError> {
        let build::Step::Script(adapter) = step else {
            panic!("expected script adapter");
        };

        // Normalize `command` field based on whether it's a single command or multiple.
        let cmds = adapter.command.as_vec();

        // Iterate over configured commands
        for input_cmd in cmds {
            // Parse command input
            let input = shellwords::split(&input_cmd).context(ScriptError::Parse {
                command: input_cmd.to_owned(),
            })?;

            // Separate command and args
            let (cmd, args) = input.split_first().context(ScriptError::InvalidCommand {
                command: input_cmd.to_owned(),
                reason: "command must include at least one element".to_string(),
            })?;

            // Try resolving the command as a local path (e.g., ./mytool)
            let cmd = match dunce::canonicalize(params.path.join(cmd)) {
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
            cmd.current_dir(&params.path);

            // Environment Variables
            cmd.env("ICP_WASM_OUTPUT_PATH", &params.output);

            // Output
            cmd.stdin(Stdio::inherit());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            // Spawn
            let mut child = cmd.spawn().context(ScriptError::Invoke {
                command: input_cmd.to_owned(),
            })?;

            // Stdio handles
            let (stdout, stderr) = (
                child.stdout.take().unwrap(), //
                child.stderr.take().unwrap(), //
            );

            // Create buffered line readers
            let (mut stdout, mut stderr) = (
                BufReader::new(stdout).lines(), //
                BufReader::new(stderr).lines(), //
            );

            // Spawn command and handle stdio
            // We need to join! as opposed to try_join! even if we only care about the result of the task
            // because we want to make sure we finish  reading all of the output
            let (stdout, stderr, status) = join!(
                //
                // Stdout
                tokio::spawn({
                    // Clone the stdio sender for use in the stdout handling task
                    let stdio = stdio.clone();

                    async move {
                        while let Ok(Some(line)) = stdout.next_line().await {
                            stdio.send(line).await?;
                        }
                        Ok::<(), BuildError>(())
                    }
                }),
                //
                // Stderr
                tokio::spawn({
                    // Clone the stdio sender for use in the stderr handling task
                    let stdio = stdio.clone();

                    async move {
                        while let Ok(Some(line)) = stderr.next_line().await {
                            stdio.send(line).await?;
                        }
                        Ok::<(), BuildError>(())
                    }
                }),
                //
                // Status
                tokio::spawn(async move {
                    //
                    child.wait().await
                }),
            );
            stdout??;
            stderr??;

            // Status
            let status =
                status
                    .context("failed to join futures")?
                    .context(ScriptError::Invoke {
                        command: input_cmd.to_owned(),
                    })?;

            if !status.success() {
                return Err(ScriptError::Status {
                    command: input_cmd.to_owned(),
                    code: status.code().map_or("N/A".to_string(), |c| c.to_string()),
                }
                .into());
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Synchronize for Script {
    async fn sync(
        &self,
        step: &sync::Step,
        params: &sync::Params,
        _: &Agent,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        // Adapter
        let adapter = match step {
            sync::Step::Script(v) => v,
            _ => panic!("expected script adapter"),
        };

        // Normalize `command` field based on whether it's a single command or multiple.
        let cmds = adapter.command.as_vec();

        // Iterate over configured commands
        for input_cmd in cmds {
            // Parse command input
            let input = shellwords::split(&input_cmd).context(ScriptError::Parse {
                command: input_cmd.to_owned(),
            })?;

            // Separate command and args
            let (cmd, args) = input.split_first().context(ScriptError::InvalidCommand {
                command: input_cmd.to_owned(),
                reason: "command must include at least one element".to_string(),
            })?;

            // Try resolving the command as a local path (e.g., ./mytool)
            let cmd = match dunce::canonicalize(params.path.join(cmd)) {
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
            cmd.current_dir(&params.path);

            // Output
            cmd.stdin(Stdio::inherit());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            // Spawn
            let mut child = cmd.spawn().context(ScriptError::Invoke {
                command: input_cmd.to_owned(),
            })?;

            // Stdio handles
            let (stdout, stderr) = (
                child.stdout.take().unwrap(), //
                child.stderr.take().unwrap(), //
            );

            // Create buffered line readers
            let (mut stdout, mut stderr) = (
                BufReader::new(stdout).lines(), //
                BufReader::new(stderr).lines(), //
            );

            // Spawn command and handle stdio
            // We need to join! as opposed to try_join! even if we only care about the result of the task
            // because we want to make sure we finish  reading all of the output
            let (_, _, status) = join!(
                //
                // Stdout
                tokio::spawn({
                    // Clone the stdio sender for use in the stdout handling task
                    let stdio = stdio.clone();

                    async move {
                        while let Ok(Some(line)) = stdout.next_line().await {
                            if let Some(sender) = &stdio {
                                let _ = sender.send(line).await;
                            }
                        }
                    }
                }),
                //
                // Stderr
                tokio::spawn({
                    // Clone the stdio sender for use in the stderr handling task
                    let stdio = stdio.clone();

                    async move {
                        while let Ok(Some(line)) = stderr.next_line().await {
                            if let Some(sender) = &stdio {
                                let _ = sender.send(line).await;
                            }
                        }
                    }
                }),
                //
                // Status
                tokio::spawn(async move {
                    //
                    child.wait().await
                }),
            );

            // Status
            let status =
                status
                    .context("failed to join futures")?
                    .context(ScriptError::Invoke {
                        command: input_cmd.to_owned(),
                    })?;

            if !status.success() {
                return Err(ScriptError::Status {
                    command: input_cmd.to_owned(),
                    code: status.code().map_or("N/A".to_string(), |c| c.to_string()),
                }
                .into());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use camino_tempfile::NamedUtf8TempFile;
    use tokio::sync::mpsc;

    use crate::{
        canister::{
            build::{self, Build, BuildError},
            script::{Script, ScriptError},
        },
        manifest::adapter::script::{Adapter, CommandField},
    };

    #[tokio::test]
    async fn single_command() {
        // Create temporary file
        let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

        // Define adapter
        let v = Adapter {
            command: CommandField::Command(format!(
                "sh -c 'echo test > {} && echo {}'",
                f.path(),
                f.path()
            )),
        };

        let (tx, _rx) = mpsc::channel::<String>(100);
        Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                tx,
            )
            .await
            .expect("failed to build script step");

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
        let v = Adapter {
            command: CommandField::Commands(vec![
                format!("sh -c 'echo cmd-1 >> {}'", f.path()),
                format!("sh -c 'echo cmd-2 >> {}'", f.path()),
                format!("sh -c 'echo cmd-3 >> {}'", f.path()),
                format!("echo {}", f.path()),
            ]),
        };

        let (tx, _rx) = mpsc::channel::<String>(100);
        Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                tx,
            )
            .await
            .expect("failed to build script step");

        // Verify command ran
        let mut out = String::new();

        f.read_to_string(&mut out)
            .expect("failed to read temporary file");

        assert_eq!(out, "cmd-1\ncmd-2\ncmd-3\n".to_string());
    }

    #[tokio::test]
    async fn invalid_command() {
        // Define adapter
        let v = Adapter {
            command: CommandField::Command("".into()),
        };

        let (tx, _rx) = mpsc::channel::<String>(100);
        let out = Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                tx,
            )
            .await;

        // Assert failure
        if out.is_ok() {
            panic!("expected invalid command to fail");
        }
    }

    #[tokio::test]
    async fn failed_unknown_command() {
        // Define adapter
        let v = Adapter {
            command: CommandField::Command("unknown-command".into()),
        };

        let (tx, _rx) = mpsc::channel::<String>(100);
        let out = Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                tx,
            )
            .await;

        // Assert failure
        if out.is_ok() {
            panic!("expected unknown command to fail");
        }
    }

    #[tokio::test]
    async fn failed_command_error_status() {
        // Define adapter
        let v = Adapter {
            command: CommandField::Command("sh -c 'exit 1'".into()),
        };

        let (tx, _rx) = mpsc::channel::<String>(100);
        let out = Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                tx,
            )
            .await;

        // Assert failure
        assert!(matches!(
            out,
            Err(BuildError::Script(ScriptError::Status { .. })),
        ));
    }
}
