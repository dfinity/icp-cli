use std::process::Stdio;

use async_trait::async_trait;
use ic_agent::Agent;
use snafu::prelude::*;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    join,
    process::Command,
    sync::mpsc::Sender,
};

use crate::canister::sync::{self, Synchronize, SynchronizeError};
use crate::manifest::canister::SyncStep;
use crate::prelude::*;

pub struct Script;

fn shell_command(s: &str, cwd: &Path) -> Result<Command, ScriptError> {
    let words = shellwords::split(s).map_err(|e| ScriptError::Parse {
        command: s.to_owned(),
        reason: e.to_string(),
    })?;

    if words.is_empty() {
        return EmptyCommandSnafu {
            command: s.to_owned(),
        }
        .fail();
    }

    let mut cmd = Command::new("sh");
    cmd.args(["-c", s]);
    cmd.current_dir(cwd);
    Ok(cmd)
}

#[derive(Debug, Snafu)]
pub enum ScriptError {
    #[snafu(display("failed to parse command: '{command}'"))]
    Parse { command: String, reason: String },

    #[snafu(display("command must not be empty: '{command}'"))]
    EmptyCommand { command: String },

    #[snafu(display("failed to execute command '{command}'"))]
    Invoke {
        source: tokio::io::Error,
        command: String,
    },

    #[snafu(display("failed to join command futures"))]
    Join { source: tokio::task::JoinError },

    #[snafu(display("failed to get command status for '{command}'"))]
    Child {
        source: std::io::Error,
        command: String,
    },

    #[snafu(display("command '{command}' failed with status code {code}"))]
    Status { command: String, code: String },
}

impl Script {
    async fn sync_impl(
        &self,
        step: &SyncStep,
        params: &sync::Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), ScriptError> {
        // Adapter
        let adapter = match step {
            SyncStep::Script(v) => v,
            _ => panic!("expected script adapter"),
        };

        // Normalize `command` field based on whether it's a single command or multiple.
        let cmds = adapter.command.as_vec();

        // Iterate over configured commands
        for input_cmd in cmds {
            let mut cmd = shell_command(&input_cmd, params.path.as_ref())?;

            // Output
            cmd.stdin(Stdio::inherit());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            // Spawn
            let mut child = cmd.spawn().context(InvokeSnafu {
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
            let status = status.context(JoinSnafu)?.context(ChildSnafu {
                command: input_cmd.to_owned(),
            })?;

            if !status.success() {
                return StatusSnafu {
                    command: input_cmd.to_owned(),
                    code: status.code().map_or("N/A".to_string(), |c| c.to_string()),
                }
                .fail();
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Synchronize for Script {
    async fn sync(
        &self,
        step: &SyncStep,
        params: &sync::Params,
        _: &Agent,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        Ok(self.sync_impl(step, params, stdio).await?)
    }
}
