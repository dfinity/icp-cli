use async_dropper::{AsyncDrop, AsyncDropper};
use async_trait::async_trait;
use bollard::{
    Docker,
    query_parameters::{
        CreateContainerOptions, InspectContainerOptions, RemoveContainerOptions,
        StartContainerOptions, StopContainerOptions, WaitContainerOptions,
    },
    secret::{ContainerCreateBody, HostConfig, Mount, MountTypeEnum, PortBinding},
};
use camino_tempfile::Utf8TempDir;
use candid::Principal;
use futures::TryStreamExt;
use notify::Watcher;
use serde::Deserialize;
use snafu::prelude::*;
use std::{collections::HashMap, io::ErrorKind, process::Stdio};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};
use tokio::{process::Child, select};

use crate::{network::Port, prelude::*};

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
}

pub async fn spawn_network_launcher(
    network_launcher_path: &Path,
    stdout_file: &Path,
    stderr_file: &Path,
    background: bool,
    port: &Port,
    state_dir: &Path,
) -> Result<(ChildSignalOnDrop, NetworkInstance), SpawnNetworkLauncherError> {
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
    let child = ChildSignalOnDrop { child };
    let launcher_status = watcher.await.context(WatchForStatusFileSnafu)?;
    Ok((
        child,
        NetworkInstance {
            gateway_port: launcher_status.gateway_port,
            root_key: hex::decode(&launcher_status.root_key).unwrap(),
            pocketic_config_port: launcher_status.config_port,
            pocketic_instance_id: launcher_status.instance_id,
        },
    ))
}

pub fn send_sigint(pid: Pid) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = system.process(pid) {
        process.kill_with(Signal::Interrupt);
    }
}

pub struct ChildSignalOnDrop {
    pub child: Child,
}

impl ChildSignalOnDrop {
    pub fn signal(&self) {
        if let Some(id) = self.child.id() {
            send_sigint((id as usize).into());
        }
    }
}

impl Drop for ChildSignalOnDrop {
    fn drop(&mut self) {
        self.signal();
    }
}

pub async fn spawn_docker_launcher(
    image: &str,
    port_mappings: &[String],
) -> (AsyncDropper<DockerDropGuard>, NetworkInstance) {
    let status_dir = Utf8TempDir::new().unwrap();
    #[cfg(unix)]
    let docker = Docker::connect_with_socket_defaults()
        .expect("failed to connect to docker socket (is it running?)");
    let portmap = port_mappings
        .iter()
        .map(|mapping| {
            let (host_port, container_port) = mapping
                .split_once(':')
                .expect("invalid port mapping, must be in format <host_port>:<container_port>");
            (
                format!("{}/tcp", container_port),
                Some(vec![PortBinding {
                    host_ip: None,
                    host_port: Some(host_port.to_string()),
                }]),
            )
        })
        .collect::<HashMap<_, _>>();
    let container_resp = docker
        .create_container(
            None::<CreateContainerOptions>,
            ContainerCreateBody {
                image: Some(image.to_string()),
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                host_config: Some(HostConfig {
                    port_bindings: Some(portmap),
                    mounts: Some(vec![Mount {
                        target: Some("/app/status".to_string()),
                        source: Some(status_dir.path().to_string()),
                        typ: Some(MountTypeEnum::BIND),
                        read_only: Some(false),
                        ..<_>::default()
                    }]),
                    ..<_>::default()
                }),
                ..<_>::default()
            },
        )
        .await
        .expect("failed to create docker container");
    let container_id = container_resp.id;
    let guard = AsyncDropper::new(DockerDropGuard {
        container_id: Some(container_id),
        docker: Some(docker),
    });
    let container_id = guard.container_id.as_ref().unwrap();
    let docker = guard.docker.as_ref().unwrap();
    let watcher = wait_for_single_line_file(&status_dir.path().join("status.json")).unwrap();
    docker
        .start_container(container_id, None::<StartContainerOptions>)
        .await
        .expect("failed to start docker container");
    let mut wait_container = docker.wait_container(container_id, None::<WaitContainerOptions>);
    let status_content = select! {
        content = watcher => content.unwrap(),
        res = wait_container.try_next() => {
            let exit = res.unwrap();
            if let Some(exit) = exit {
                panic!("Docker container exited with code {} before writing status file.", exit.status_code);
            } else {
                panic!("Docker container exited before writing status file.");
            }
        },
    };
    let launcher_status: LauncherStatus =
        serde_json::from_str(&status_content).expect("failed to parse launcher status file");
    assert_eq!(
        launcher_status.v, "1",
        "unexpected Docker launcher status version"
    );
    let container_info = docker
        .inspect_container(container_id, None::<InspectContainerOptions>)
        .await
        .expect("failed to inspect docker container");
    let container_config_port = launcher_status.config_port;
    let container_gateway_port = launcher_status.gateway_port;
    let port_bindings = container_info
        .network_settings
        .expect("missing network settings in docker container")
        .ports
        .expect("missing port mappings in docker container");
    let host_config_port = container_config_port.map(|container_config_port| {
        port_bindings
            .get(&format!("{container_config_port}/tcp"))
            .expect("missing PIC config port in docker container")
            .as_ref()
            .expect("missing host port binding for PIC config port in docker container")
            .first()
            .expect("missing host port binding for PIC config port in docker container")
            .host_port
            .as_ref()
            .expect("missing host port for PIC config port in docker container")
            .parse::<u16>()
            .expect("invalid host port for PIC config port in docker container")
    });
    let host_gateway_port = port_bindings
        .get(&format!("{container_gateway_port}/tcp"))
        .expect("missing PIC gateway port in docker container")
        .as_ref()
        .expect("missing host port binding for PIC gateway port in docker container")
        .first()
        .expect("missing host port binding for PIC gateway port in docker container")
        .host_port
        .as_ref()
        .expect("missing host port for PIC gateway port in docker container")
        .parse::<u16>()
        .expect("invalid host port for PIC gateway port in docker container");
    (
        guard,
        NetworkInstance {
            gateway_port: host_gateway_port,
            pocketic_config_port: host_config_port,
            pocketic_instance_id: launcher_status.instance_id,
            root_key: hex::decode(&launcher_status.root_key)
                .expect("invalid root key in launcher status"),
        },
    )
}

#[derive(Default)]
pub struct DockerDropGuard {
    docker: Option<Docker>,
    container_id: Option<String>,
}

#[async_trait]
impl AsyncDrop for DockerDropGuard {
    async fn async_drop(&mut self) {
        if let Some(docker) = &self.docker.take() {
            let container_id = self.container_id.take().unwrap();
            let _ = docker
                .stop_container(&container_id, None::<StopContainerOptions>)
                .await;
            let _ = docker
                .remove_container(&container_id, None::<RemoveContainerOptions>)
                .await;
        }
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
