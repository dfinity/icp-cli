use std::process::Stdio;

use snafu::prelude::*;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    join,
    process::Command,
    sync::mpsc::Sender,
};

use crate::manifest::adapter::script::Adapter;
use crate::prelude::*;

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

    #[cfg(windows)]
    #[snafu(display(
        "failed to locate bash (the git at {git_path} does not appear to be Git for Windows, try running in Git Bash)"
    ))]
    LocateBash { git_path: PathBuf },

    #[cfg(windows)]
    #[snafu(display("failed to locate git executable in PATH (try running in Git Bash)"))]
    LocateGit,

    #[cfg(windows)]
    #[snafu(display("unprocessable executable path: {}", path.display()))]
    BadPath {
        path: std::path::PathBuf,
        source: camino::FromPathBufError,
    },
}

pub(super) async fn execute(
    adapter: &Adapter,
    cwd: &Path,
    envs: &[(&str, &str)],
    stdio: Option<Sender<String>>,
) -> Result<(), ScriptError> {
    // Normalize `command` field based on whether it's a single command or multiple.
    let cmds = adapter.command.as_vec();

    // Iterate over configured commands
    for input_cmd in cmds {
        let mut cmd = shell_command(&input_cmd, cwd)?;

        // Environment Variables
        for env in envs {
            cmd.env(env.0, env.1);
        }
        // cmd.env("ICP_WASM_OUTPUT_PATH", &params.output);

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
                    Ok::<(), ScriptError>(())
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
                    Ok::<(), ScriptError>(())
                }
            }),
            //
            // Status
            tokio::spawn(async move {
                //
                child.wait().await
            }),
        );
        stdout.context(JoinSnafu)??;
        stderr.context(JoinSnafu)??;

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
    #[cfg(unix)]
    let mut cmd = Command::new("sh");
    #[cfg(windows)]
    let mut cmd = if let Some(_) = std::env::var_os("MSYSTEM") {
        Command::new("bash")
    } else {
        use winreg::{RegKey, enums::*};
        let git_for_windows_path = if let Ok(lm_path) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\GitForWindows")
            .and_then(|key| key.get_value::<String, _>("InstallPath"))
        {
            lm_path
        } else if let Ok(cu_path) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"SOFTWARE\GitForWindows")
            .and_then(|key| key.get_value::<String, _>("InstallPath"))
        {
            cu_path
        } else {
            return LocateGitSnafu.fail();
        };
        Command::new(git_for_windows_path.join("bin/bash.exe"))
    };
    cmd.args(["-c", s]);
    cmd.current_dir(cwd);
    Ok(cmd)
}
