use std::process::Stdio;

use crate::prelude::*;
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

/// Creates a Command that either runs directly or through a shell.
///
/// If the command is a single word pointing to an executable file, it runs directly.
/// Otherwise, it automatically wraps the command in `sh -c` to support shell features
/// like pipes, redirections, and multiple commands.
fn direct_or_shell_command(s: &str, cwd: &Path) -> anyhow::Result<Command> {
    let words = shellwords::split(s).with_context(|| format!("Cannot parse command '{s}'."))?;

    if words.is_empty() {
        anyhow::bail!("Command must include at least one element");
    }

    let canonical_result = dunce::canonicalize(cwd.join(&words[0]));
    let mut cmd = if words.len() == 1 && canonical_result.is_ok() {
        // If the command is a single word pointing to a file, execute it directly.
        #[allow(clippy::unnecessary_unwrap)]
        let file = canonical_result.unwrap();
        Command::new(file)
    } else {
        // Execute the command in `sh -c` to allow pipes, redirections, etc.
        let mut sh_cmd = Command::new("sh");
        sh_cmd.args(["-c", s]);
        sh_cmd
    };
    cmd.current_dir(cwd);
    Ok(cmd)
}

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
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        let build::Step::Script(adapter) = step else {
            panic!("expected script adapter");
        };

        // Normalize `command` field based on whether it's a single command or multiple.
        let cmds = adapter.command.as_vec();

        // Iterate over configured commands
        for input_cmd in cmds {
            let mut cmd = direct_or_shell_command(&input_cmd, params.path.as_ref()).context(
                ScriptError::Parse {
                    command: input_cmd.to_owned(),
                },
            )?;

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
                            if let Some(sender) = &stdio {
                                let _ = sender.send(line).await;
                            }
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
                            if let Some(sender) = &stdio {
                                let _ = sender.send(line).await;
                            }
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
            let mut cmd = direct_or_shell_command(&input_cmd, params.path.as_ref()).context(
                ScriptError::Parse {
                    command: input_cmd.to_owned(),
                },
            )?;

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
                "echo test > {} && echo {}",
                f.path(),
                f.path()
            )),
        };

        Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                None,
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
                format!("echo cmd-1 >> {}", f.path()),
                format!("echo cmd-2 >> {}", f.path()),
                format!("echo cmd-3 >> {}", f.path()),
                format!("echo {}", f.path()),
            ]),
        };

        Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                None,
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

        let out = Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                None,
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

        let out = Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                None,
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
            command: CommandField::Command("exit 1".into()),
        };

        let out = Script
            .build(
                &build::Step::Script(v),
                &build::Params {
                    path: "/".into(),
                    output: "/".into(),
                },
                None,
            )
            .await;

        // Assert failure
        assert!(matches!(
            out,
            Err(BuildError::Script(ScriptError::Status { .. })),
        ));
    }
}
