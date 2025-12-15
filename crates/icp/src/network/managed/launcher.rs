use async_dropper::{AsyncDrop, AsyncDropper};
use async_trait::async_trait;
use camino_tempfile::Utf8TempDir;
use candid::Principal;
use notify::Watcher;
use serde::Deserialize;
use snafu::prelude::*;
use std::{io::ErrorKind, process::Stdio};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};
use tokio::{process::Child, select};

use crate::{
    network::{Port, config::ChildLocator},
    prelude::*,
};

pub struct NetworkInstance {
    pub gateway_port: u16,
    pub root_key: Vec<u8>,
    pub pocketic_config_port: Option<u16>,
    pub pocketic_instance_id: Option<usize>,
}

#[derive(Debug, Snafu)]
pub enum SpawnNetworkLauncherError {
    #[snafu(display("failed to create status directory"))]
    CreateStatusDir { source: std::io::Error },
    #[snafu(display("failed to create stdio log at {path}"))]
    CreateStdioFile {
        source: std::io::Error,
        path: PathBuf,
    },
    #[snafu(display("failed to watch status directory"))]
    WatchStatusDir { source: WaitForFileError },
    #[snafu(display("failed to spawn network launcher {network_launcher_path}"))]
    SpawnLauncher {
        source: std::io::Error,
        network_launcher_path: PathBuf,
    },
    #[snafu(display("failed to watch launcher status file"))]
    WatchForStatusFile { source: WaitForLauncherStatusError },
    #[snafu(display(
        "network launcher at {network_launcher_path} exited prematurely with status {exit_status}"
    ))]
    LauncherExitedPrematurely {
        network_launcher_path: PathBuf,
        exit_status: std::process::ExitStatus,
    },
    #[snafu(display("failed to watch launcher process for exit code"))]
    WatchLauncher {
        network_launcher_path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to parse root key {key}"))]
    ParseRootKey {
        key: String,
        source: hex::FromHexError,
    },
}

pub async fn spawn_network_launcher(
    network_launcher_path: &Path,
    stdout_file: &Path,
    stderr_file: &Path,
    background: bool,
    port: &Port,
    state_dir: &Path,
) -> Result<
    (
        AsyncDropper<ChildSignalOnDrop>,
        NetworkInstance,
        ChildLocator,
    ),
    SpawnNetworkLauncherError,
> {
    let mut cmd = tokio::process::Command::new(network_launcher_path);
    cmd.args([
        "--interface-version",
        "1.0.0",
        "--state-dir",
        state_dir.as_str(),
        "--ii",
    ]);
    if let Port::Fixed(port) = port {
        cmd.args(["--gateway-port", &port.to_string()]);
    }
    let status_dir = Utf8TempDir::new().context(CreateStatusDirSnafu)?;
    cmd.args(["--status-dir", status_dir.path().as_str()]);
    if background {
        eprintln!("For background mode, network output will be redirected:");
        eprintln!("  stdout: {}", stdout_file);
        eprintln!("  stderr: {}", stderr_file);
        let stdout = std::fs::File::create(stdout_file)
            .context(CreateStdioFileSnafu { path: &stdout_file })?;
        let stderr = std::fs::File::create(stderr_file)
            .context(CreateStdioFileSnafu { path: &stderr_file })?;
        cmd.stdout(Stdio::from(stdout));
        cmd.stderr(Stdio::from(stderr));
    } else {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }
    let watcher = wait_for_launcher_status(status_dir.as_ref()).context(WatchStatusDirSnafu)?;
    let child = cmd.spawn().context(SpawnLauncherSnafu {
        network_launcher_path,
    })?;
    let mut guard = AsyncDropper::new(ChildSignalOnDrop { child: Some(child) });
    let child = guard.child.as_mut().unwrap();
    let launcher_status = select! {
        status = watcher => status.context(WatchForStatusFileSnafu)?,
        // If the child process exits before writing the status file, return an error.
        res = child.wait() => {
            let exit_status = res.context(WatchLauncherSnafu {
                network_launcher_path,
            })?;
            return LauncherExitedPrematurelySnafu {
                exit_status,
                network_launcher_path: &network_launcher_path,
            }.fail();
        },
    };
    Ok((
        guard,
        NetworkInstance {
            gateway_port: launcher_status.gateway_port,
            root_key: hex::decode(&launcher_status.root_key).context(ParseRootKeySnafu {
                key: &launcher_status.root_key,
            })?,
            pocketic_config_port: launcher_status.config_port,
            pocketic_instance_id: launcher_status.instance_id,
        },
        ChildLocator::Pid(launcher_status.instance_id.unwrap() as u32),
    ))
}

pub fn stop_launcher(pid: Pid) {
    send_sigint(pid);
}

pub fn send_sigint(pid: Pid) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = system.process(pid) {
        process.kill_with(Signal::Interrupt);
    }
}

#[derive(Default)]
pub struct ChildSignalOnDrop {
    pub child: Option<Child>,
}

impl ChildSignalOnDrop {
    pub async fn signal_and_wait(&mut self) -> std::io::Result<()> {
        if let Some(mut child) = self.child.take()
            && let Some(id) = child.id()
        {
            send_sigint((id as usize).into());
            child.wait().await?;
        }
        Ok(())
    }
    pub fn defuse(&mut self) {
        self.child = None;
    }
}

#[async_trait]
impl AsyncDrop for ChildSignalOnDrop {
    async fn async_drop(&mut self) {
        _ = self.signal_and_wait().await;
    }
}

#[derive(Debug, Snafu)]
pub enum WaitForFileError {
    #[snafu(display("failed to watch file changes at path {path}"))]
    Watch {
        source: notify::Error,
        path: PathBuf,
    },

    #[snafu(display("failed to read event for file {path}"))]
    ReadEvent {
        source: notify::Error,
        path: PathBuf,
    },

    #[snafu(transparent)]
    ReadFile { source: crate::fs::IoError },
}

/// Waits for a file to be created and have a full line of content. Call the function before initing the external process,
/// then await the future after the init.
pub fn wait_for_single_line_file(
    path: &Path,
) -> Result<impl Future<Output = Result<String, WaitForFileError>> + use<>, WaitForFileError> {
    let dir = path.parent().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut tx = Some(tx);
    let mut watcher = notify::recommended_watcher({
        let path = path.to_path_buf();
        let dir = dir.to_path_buf();
        move |res: notify::Result<notify::Event>| match res {
            Ok(event) => {
                if event.kind.is_modify() {
                    let content_res = crate::fs::read_to_string(&path);
                    match content_res {
                        Ok(content) => {
                            if content.ends_with('\n')
                                && let Some(tx) = tx.take()
                            {
                                let _ = tx.send(Ok(content));
                            }
                        }
                        Err(e) if e.kind() == ErrorKind::NotFound => {}
                        Err(e) => {
                            if let Some(tx) = tx.take() {
                                let _ = tx.send(Err(e.into()));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                if let Some(tx) = tx.take() {
                    let _ = tx.send(Err(e).context(ReadEventSnafu { path: &dir }));
                }
            }
        }
    })
    .context(WatchSnafu { path: &dir })?;
    watcher
        .watch(dir.as_std_path(), notify::RecursiveMode::NonRecursive)
        .context(WatchSnafu { path: &dir })?;
    Ok(async {
        let _watcher = watcher;
        let res = rx.await;
        res.unwrap()
    })
}

/// Call the function before initing the external process, then await the future after the init.
pub fn wait_for_launcher_status(
    status_dir: &Path,
) -> Result<
    impl Future<Output = Result<LauncherStatus, WaitForLauncherStatusError>> + use<>,
    WaitForFileError,
> {
    let status_file = status_dir.join("status.json");
    let watcher = wait_for_single_line_file(&status_file)?;
    Ok(async move {
        let status_content = watcher.await.context(WaitForFileSnafu)?;
        let launcher_status: LauncherStatus =
            serde_json::from_str(&status_content).context(DeserializeSnafu)?;
        ensure!(
            launcher_status.v == "1",
            BadVersionSnafu {
                expected: "1",
                found: &launcher_status.v
            }
        );
        Ok(launcher_status)
    })
}

#[derive(Debug, Snafu)]
pub enum WaitForLauncherStatusError {
    WaitForFile { source: WaitForFileError },
    Deserialize { source: serde_json::Error },
    BadVersion { expected: String, found: String },
}

#[derive(Deserialize)]
pub struct LauncherStatus {
    pub v: String,
    pub instance_id: Option<usize>,
    pub config_port: Option<u16>,
    pub gateway_port: u16,
    pub root_key: String,
    pub default_effective_canister_id: Option<Principal>,
}

#[derive(Debug, Snafu)]
pub enum CreateHttpGatewayError {
    #[snafu(
        display("failed to create HTTP gateway: {message}"),
        context(suffix(GatewaySnafu))
    )]
    Create { message: String },

    #[snafu(transparent, context(false))]
    Reqwest { source: reqwest::Error },
}
