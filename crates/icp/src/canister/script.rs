use async_trait::async_trait;

use crate::canister::{
    build::{self, Build, BuildError},
    sync::{self, Synchronize, SynchronizeError},
};

pub struct Script;

#[async_trait]
impl Build for Script {
    async fn build(&self, step: build::Step) -> Result<(), BuildError> {
        Ok(())
    }
}

#[async_trait]
impl Synchronize for Script {
    async fn sync(&self, step: sync::Step) -> Result<(), SynchronizeError> {
        Ok(())
    }
}

// use std::fmt;
// use std::process::Stdio;

// use async_trait::async_trait;
// use ic_agent::{Agent, export::Principal};
// use icp::prelude::*;
// use schemars::JsonSchema;
// use serde::Deserialize;
// use snafu::{OptionExt, ResultExt, Snafu};
// use tokio::{
//     io::{AsyncBufReadExt, BufReader},
//     join,
//     process::Command,
//     sync::mpsc::Sender,
// };

// use crate::build::{self, AdapterCompileError};
// use crate::sync::{self, AdapterSyncError};

// #[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
// #[serde(rename_all = "lowercase")]
// pub enum CommandField {
//     /// Command used to build a canister
//     Command(String),

//     /// Set of commands used to build a canister
//     Commands(Vec<String>),
// }

// impl CommandField {
//     fn as_vec(&self) -> Vec<String> {
//         match self {
//             Self::Command(cmd) => vec![cmd.clone()],
//             Self::Commands(cmds) => cmds.clone(),
//         }
//     }
// }

// /// Configuration for a custom canister build adapter.
// #[derive(Clone, Debug, Deserialize, JsonSchema)]
// pub struct ScriptAdapter {
//     /// Command used to build a canister
//     #[serde(flatten)]
//     pub command: CommandField,

//     #[serde(skip)]
//     pub stdio_sender: Option<Sender<String>>,
// }

// impl ScriptAdapter {
//     pub fn with_stdio_sender(&self, sender: Sender<String>) -> Self {
//         let mut v = self.clone();
//         v.stdio_sender = Some(sender);
//         v
//     }
// }

// impl PartialEq for ScriptAdapter {
//     fn eq(&self, other: &Self) -> bool {
//         self.command == other.command
//     }
// }

// impl fmt::Display for ScriptAdapter {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         let cmd = match &self.command {
//             CommandField::Command(c) => format!("command: {c}"),
//             CommandField::Commands(cs) => format!("{} commands", cs.len()),
//         };

//         write!(f, "({cmd})")
//     }
// }

// #[async_trait]
// impl build::Adapter for ScriptAdapter {
//     async fn compile(
//         &self,
//         canister_path: &Path,
//         wasm_output_path: &Path,
//     ) -> Result<(), AdapterCompileError> {
//         // Normalize `command` field based on whether it's a single command or multiple.
//         let cmds = self.command.as_vec();

//         // Iterate over configured commands
//         for input_cmd in cmds {
//             // Parse command input
//             let input = shellwords::split(&input_cmd).context(CommandParseCompileSnafu {
//                 command: &input_cmd,
//             })?;

//             // Separate command and args
//             let (cmd, args) = input.split_first().context(InvalidCommandCompileSnafu {
//                 command: &input_cmd,
//                 reason: "command must include at least one element".to_string(),
//             })?;

//             // Try resolving the command as a local path (e.g., ./mytool)
//             let cmd = match dunce::canonicalize(canister_path.join(cmd)) {
//                 // Use the canonicalized local path if it exists
//                 Ok(p) => p,

//                 // Fall back to assuming it's a command in the system PATH
//                 Err(_) => cmd.into(),
//             };

//             // Command
//             let mut cmd = Command::new(cmd);

//             // Args
//             cmd.args(args);

//             // Set directory
//             cmd.current_dir(canister_path);

//             // Environment Variables
//             cmd.env("ICP_WASM_OUTPUT_PATH", wasm_output_path);

//             // Output
//             cmd.stdin(Stdio::inherit());
//             cmd.stdout(Stdio::piped());
//             cmd.stderr(Stdio::piped());

//             // Spawn
//             let mut child = cmd.spawn().context(CommandInvokeCompileSnafu {
//                 command: &input_cmd,
//             })?;

//             // Stdio handles
//             let (stdout, stderr) = (
//                 child.stdout.take().unwrap(), //
//                 child.stderr.take().unwrap(), //
//             );

//             // Create buffered line readers
//             let (mut stdout, mut stderr) = (
//                 BufReader::new(stdout).lines(), //
//                 BufReader::new(stderr).lines(), //
//             );

//             // Spawn command and handle stdio
//             // We need to join! as opposed to try_join! even if we only care about the result of the task
//             // because we want to make sure we finish  reading all of the output
//             let (_, _, status) = join!(
//                 //
//                 // Stdout
//                 tokio::spawn({
//                     // Clone the stdio sender for use in the stdout handling task
//                     let stdio_sender = self.stdio_sender.clone();

//                     async move {
//                         while let Ok(Some(line)) = stdout.next_line().await {
//                             if let Some(sender) = &stdio_sender {
//                                 let _ = sender.send(line).await;
//                             }
//                         }
//                     }
//                 }),
//                 //
//                 // Stderr
//                 tokio::spawn({
//                     // Clone the stdio sender for use in the stderr handling task
//                     let stdio_sender = self.stdio_sender.clone();

//                     async move {
//                         while let Ok(Some(line)) = stderr.next_line().await {
//                             if let Some(sender) = &stdio_sender {
//                                 let _ = sender.send(line).await;
//                             }
//                         }
//                     }
//                 }),
//                 //
//                 // Status
//                 tokio::spawn(async move {
//                     //
//                     child.wait().await
//                 }),
//             );

//             // Status
//             let status = status.context(JoinCompileSnafu)?;
//             let status = status.context(CommandInvokeCompileSnafu {
//                 command: &input_cmd,
//             })?;

//             if !status.success() {
//                 return Err(ScriptAdapterCompileError::CommandStatus {
//                     command: input_cmd,
//                     code: status.code().map_or("N/A".to_string(), |c| c.to_string()),
//                 }
//                 .into());
//             }
//         }

//         Ok(())
//     }
// }

// #[derive(Debug, Snafu)]
// #[snafu(context(suffix(CompileSnafu)))]
// pub enum ScriptAdapterCompileError {
//     #[snafu(display("failed to parse command '{command}'"))]
//     CommandParse {
//         command: String,
//         source: shellwords::MismatchedQuotes,
//     },

//     #[snafu(display("invalid command '{command}': {reason}"))]
//     InvalidCommand { command: String, reason: String },

//     #[snafu(display("failed to join thread handles"))]
//     Join { source: tokio::task::JoinError },

//     #[snafu(display("failed to execute command '{command}'"))]
//     CommandInvoke {
//         command: String,
//         source: std::io::Error,
//     },

//     #[snafu(display("command '{command}' failed with status code {code}"))]
//     CommandStatus { command: String, code: String },
// }

// #[async_trait]
// impl sync::Adapter for ScriptAdapter {
//     async fn sync(
//         &self,
//         canister_path: &Path,
//         _canister_id: &Principal,
//         _agent: &Agent,
//     ) -> Result<(), AdapterSyncError> {
//         // Normalize `command` field based on whether it's a single command or multiple.
//         let cmds = self.command.as_vec();

//         // Iterate over configured commands
//         for input_cmd in cmds {
//             // Parse command input
//             let input = shellwords::split(&input_cmd).context(CommandParseSyncSnafu {
//                 command: &input_cmd,
//             })?;

//             // Separate command and args
//             let (cmd, args) = input.split_first().context(InvalidCommandSyncSnafu {
//                 command: &input_cmd,
//                 reason: "command must include at least one element".to_string(),
//             })?;

//             // Try resolving the command as a local path (e.g., ./mytool)
//             let cmd = match dunce::canonicalize(canister_path.join(cmd)) {
//                 // Use the canonicalized local path if it exists
//                 Ok(p) => p,

//                 // Fall back to assuming it's a command in the system PATH
//                 Err(_) => cmd.into(),
//             };

//             // Command
//             let mut cmd = Command::new(cmd);

//             // Args
//             cmd.args(args);

//             // Set directory
//             cmd.current_dir(canister_path);

//             // Output
//             cmd.stdin(Stdio::inherit());
//             cmd.stdout(Stdio::piped());
//             cmd.stderr(Stdio::piped());

//             // Spawn
//             let mut child = cmd.spawn().context(CommandInvokeSyncSnafu {
//                 command: &input_cmd,
//             })?;

//             // Stdio handles
//             let (stdout, stderr) = (
//                 child.stdout.take().unwrap(), //
//                 child.stderr.take().unwrap(), //
//             );

//             // Create buffered line readers
//             let (mut stdout, mut stderr) = (
//                 BufReader::new(stdout).lines(), //
//                 BufReader::new(stderr).lines(), //
//             );

//             // Spawn command and handle stdio
//             let (_, _, status) = join!(
//                 //
//                 // Stdout
//                 tokio::spawn({
//                     // Clone the stdio sender for use in the stdout handling task
//                     let stdio_sender = self.stdio_sender.clone();

//                     async move {
//                         while let Ok(Some(line)) = stdout.next_line().await {
//                             if let Some(sender) = &stdio_sender {
//                                 let _ = sender.send(line).await;
//                             }
//                         }
//                     }
//                 }),
//                 //
//                 // Stderr
//                 tokio::spawn({
//                     // Clone the stdio sender for use in the stderr handling task
//                     let stdio_sender = self.stdio_sender.clone();

//                     async move {
//                         while let Ok(Some(line)) = stderr.next_line().await {
//                             if let Some(sender) = &stdio_sender {
//                                 let _ = sender.send(line).await;
//                             }
//                         }
//                     }
//                 }),
//                 //
//                 // Status
//                 tokio::spawn(async move {
//                     //
//                     child.wait().await
//                 }),
//             );

//             // Status
//             // We need to join! as opposed to try_join! even if we only care about
//             // the task because we want to make sure we finish reading all of the output
//             let status = status.context(JoinSyncSnafu)?;
//             let status = status.context(CommandInvokeSyncSnafu {
//                 command: &input_cmd,
//             })?;

//             if !status.success() {
//                 return Err(ScriptAdapterSyncError::CommandStatus {
//                     command: input_cmd,
//                     code: status.code().map_or("N/A".to_string(), |c| c.to_string()),
//                 }
//                 .into());
//             }
//         }

//         Ok(())
//     }
// }

// #[derive(Debug, Snafu)]
// #[snafu(context(suffix(SyncSnafu)))]
// pub enum ScriptAdapterSyncError {
//     #[snafu(display("failed to parse command '{command}'"))]
//     CommandParse {
//         command: String,
//         source: shellwords::MismatchedQuotes,
//     },

//     #[snafu(display("invalid command '{command}': {reason}"))]
//     InvalidCommand { command: String, reason: String },

//     #[snafu(display("failed to join thread handles"))]
//     Join { source: tokio::task::JoinError },

//     #[snafu(display("failed to execute command '{command}'"))]
//     CommandInvoke {
//         command: String,
//         source: std::io::Error,
//     },

//     #[snafu(display("command '{command}' failed with status code {code}"))]
//     CommandStatus { command: String, code: String },
// }

// #[cfg(test)]
// mod tests {
//     use crate::build::Adapter as _;

//     use super::*;
//     use camino_tempfile::NamedUtf8TempFile;
//     use std::io::Read;

//     #[tokio::test]
//     async fn single_command() {
//         // Create temporary file
//         let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

//         // Define adapter
//         let v = ScriptAdapter {
//             command: CommandField::Command(format!(
//                 "sh -c 'echo test > {} && echo {}'",
//                 f.path(),
//                 f.path()
//             )),
//             stdio_sender: None,
//         };

//         // Invoke adapter
//         v.compile("/".into(), "/".into())
//             .await
//             .expect("unexpected failure");

//         // Verify command ran
//         let mut out = String::new();

//         f.read_to_string(&mut out)
//             .expect("failed to read temporary file");

//         assert_eq!(out, "test\n".to_string());
//     }

//     #[tokio::test]
//     async fn multiple_commands() {
//         // Create temporary file
//         let mut f = NamedUtf8TempFile::new().expect("failed to create temporary file");

//         // Define adapter
//         let v = ScriptAdapter {
//             command: CommandField::Commands(vec![
//                 format!("sh -c 'echo cmd-1 >> {}'", f.path()),
//                 format!("sh -c 'echo cmd-2 >> {}'", f.path()),
//                 format!("sh -c 'echo cmd-3 >> {}'", f.path()),
//                 format!("echo {}", f.path()),
//             ]),
//             stdio_sender: None,
//         };

//         // Invoke adapter
//         v.compile("/".into(), "/".into())
//             .await
//             .expect("unexpected failure");

//         // Verify command ran
//         let mut out = String::new();

//         f.read_to_string(&mut out)
//             .expect("failed to read temporary file");

//         assert_eq!(out, "cmd-1\ncmd-2\ncmd-3\n".to_string());
//     }

//     #[tokio::test]
//     async fn invalid_command() {
//         // Define adapter
//         let v = ScriptAdapter {
//             command: CommandField::Command("".into()),
//             stdio_sender: None,
//         };

//         // Invoke adapter
//         let out = v.compile("/".into(), "/".into()).await;

//         // Assert failure
//         assert!(matches!(
//             out,
//             Err(AdapterCompileError::Script {
//                 source: ScriptAdapterCompileError::InvalidCommand { .. }
//             })
//         ));
//     }

//     #[tokio::test]
//     async fn failed_command_not_found() {
//         // Define adapter
//         let v = ScriptAdapter {
//             command: CommandField::Command("invalid-command".into()),
//             stdio_sender: None,
//         };

//         // Invoke adapter
//         let out = v.compile("/".into(), "/".into()).await;

//         println!("{out:?}");

//         // Assert failure
//         assert!(matches!(
//             out,
//             Err(AdapterCompileError::Script {
//                 source: ScriptAdapterCompileError::CommandInvoke { .. }
//             })
//         ));
//     }

//     #[tokio::test]
//     async fn failed_command_error_status() {
//         // Define adapter
//         let v = ScriptAdapter {
//             command: CommandField::Command("sh -c 'exit 1'".into()),
//             stdio_sender: None,
//         };

//         // Invoke adapter
//         let out = v.compile("/".into(), "/".into()).await;

//         println!("{out:?}");

//         // Assert failure
//         assert!(matches!(
//             out,
//             Err(AdapterCompileError::Script {
//                 source: ScriptAdapterCompileError::CommandStatus { .. }
//             })
//         ));
//     }
// }
