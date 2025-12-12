use std::collections::HashMap;

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
use futures::TryStreamExt;
use itertools::Itertools;
#[cfg(unix)]
use snafu::ResultExt;
use snafu::{OptionExt, Snafu};
use tokio::select;

use crate::network::managed::launcher::{NetworkInstance, wait_for_launcher_status};
use crate::prelude::*;

pub async fn spawn_docker_launcher(
    image: &str,
    port_mappings: &[String],
) -> Result<(AsyncDropper<DockerDropGuard>, NetworkInstance), DockerLauncherError> {
    let status_dir = Utf8TempDir::new().context(CreateStatusDirSnafu)?;
    #[cfg(unix)]
    let docker = Docker::connect_with_unix_defaults().context(ConnectDockerSnafu {
        socket: "/var/run/docker.sock",
    })?;
    #[cfg(windows)]
    let docker = Docker::connect_with_named_pipe_defaults().context(ConnectDockerSnafu {
        socket: r"\\.\pipe\docker_engine",
    })?;
    let portmap: HashMap<_, _> = port_mappings
        .iter()
        .map(|mapping| {
            let (host_port, container_port) =
                mapping.split_once(':').context(ParsePortmapSnafu {
                    port_mapping: mapping,
                })?;
            Ok::<_, DockerLauncherError>((
                format!("{}/tcp", container_port),
                Some(vec![PortBinding {
                    host_ip: None,
                    host_port: Some(host_port.to_string()),
                }]),
            ))
        })
        .try_collect()?;
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
        .context(CreateContainerSnafu { image_name: image })?;
    let container_id = container_resp.id;
    let guard = AsyncDropper::new(DockerDropGuard {
        container_id: Some(container_id),
        docker: Some(docker),
    });
    let container_id = guard.container_id.as_ref().unwrap();
    let docker = guard.docker.as_ref().unwrap();
    let watcher = wait_for_launcher_status(status_dir.path())?;
    docker
        .start_container(container_id, None::<StartContainerOptions>)
        .await
        .context(StartContainerSnafu { container_id })?;
    let mut wait_container = docker.wait_container(container_id, None::<WaitContainerOptions>);
    let launcher_status = select! {
        content = watcher => content?,
        res = wait_container.try_next() => {
            let exit = res.context(WatchContainerSnafu { container_id })?;
            if let Some(exit) = exit {
                return ContainerExitedPrematurelySnafu {
                    container_id,
                    exit_status: exit.status_code,
                }.fail();
            } else {
                return RequiredFieldMissingSnafu {
                    field: "StatusCode",
                    route: "wait_container",
                }.fail()
            }
        },
    };
    let container_info = docker
        .inspect_container(container_id, None::<InspectContainerOptions>)
        .await
        .context(InspectContainerSnafu { container_id })?;
    let container_config_port = launcher_status.config_port;
    let container_gateway_port = launcher_status.gateway_port;
    let port_bindings = container_info
        .network_settings
        .context(RequiredFieldMissingSnafu {
            field: "NetworkSettings",
            route: "inspect_container",
        })?
        .ports
        .context(RequiredFieldMissingSnafu {
            field: "NetworkSettings.Ports",
            route: "inspect_container",
        })?;
    let host_config_port = if let Some(container_config_port) = container_config_port {
        if let Some(port_binding) = port_bindings
            .get(&format!("{container_config_port}/tcp"))
            .and_then(|pb| pb.as_ref())
            .and_then(|pb| pb.first())
        {
            let host_port_str =
                port_binding
                    .host_port
                    .as_ref()
                    .with_context(|| RequiredFieldMissingSnafu {
                        field: format!(
                            "NetworkSettings.Ports[{container_config_port}][0].HostPort"
                        ),
                        route: "inspect_container",
                    })?;
            Some(host_port_str.parse::<u16>().context(ParsePortSnafu {
                host_port: host_port_str,
                container_port: container_config_port,
                container_id,
            })?)
        } else {
            None
        }
    } else {
        None
    };
    let host_gateway_port_str = port_bindings
        .get(&format!("{container_gateway_port}/tcp"))
        .and_then(|pb| pb.as_ref())
        .and_then(|pb| pb.first())
        .context(GatewayPortNotMappedSnafu {
            container_port: container_gateway_port,
            container_id,
        })?
        .host_port
        .as_ref()
        .with_context(|| RequiredFieldMissingSnafu {
            field: format!("NetworkSettings.Ports[{container_gateway_port}][0].HostPort"),
            route: "inspect_container",
        })?;
    let host_gateway_port = host_gateway_port_str
        .parse::<u16>()
        .context(ParsePortSnafu {
            host_port: host_gateway_port_str,
            container_port: container_gateway_port,
            container_id,
        })?;
    Ok((
        guard,
        NetworkInstance {
            gateway_port: host_gateway_port,
            pocketic_config_port: host_config_port,
            pocketic_instance_id: launcher_status.instance_id,
            root_key: hex::decode(&launcher_status.root_key).context(ParseRootKeySnafu {
                key: &launcher_status.root_key,
            })?,
        },
    ))
}

#[derive(Debug, Snafu)]
pub enum DockerLauncherError {
    #[snafu(display("failed to connect to docker daemon at {socket} (is it running?)"))]
    ConnectDocker {
        source: bollard::errors::Error,
        socket: PathBuf,
    },
    #[snafu(display("failed to create docker container"))]
    CreateContainer {
        source: bollard::errors::Error,
        image_name: String,
    },
    #[snafu(display("failed to start docker container"))]
    StartContainer {
        source: bollard::errors::Error,
        container_id: String,
    },
    #[snafu(display("failed to inspect docker container"))]
    InspectContainer {
        source: bollard::errors::Error,
        container_id: String,
    },
    #[snafu(display("failed to parse root key {key}"))]
    ParseRootKey {
        key: String,
        source: hex::FromHexError,
    },
    #[snafu(display("required field {field} in docker API route {route} is missing"))]
    RequiredFieldMissing { route: String, field: String },
    #[snafu(display(
        "container {container_id} did not map container port {container_port}/tcp to a host port"
    ))]
    GatewayPortNotMapped {
        container_port: u16,
        container_id: String,
    },
    #[snafu(display(
        "failed to parse port mapping {host_port} as number for container port {container_port}/tcp in container {container_id}"
    ))]
    ParsePort {
        source: std::num::ParseIntError,
        host_port: String,
        container_port: u16,
        container_id: String,
    },
    #[snafu(display(
        "failed to parse port mapping {port_mapping}, must be in format <host_port>:<container_port>"
    ))]
    ParsePortmap { port_mapping: String },
    #[snafu(transparent)]
    WaitForLauncherStatus {
        source: crate::network::managed::launcher::WaitForLauncherStatusError,
    },
    #[snafu(transparent)]
    WatchStatusDir {
        source: crate::network::managed::launcher::WaitForFileError,
    },
    #[snafu(display("failed to watch docker container {container_id} for exit"))]
    WatchContainer {
        source: bollard::errors::Error,
        container_id: String,
    },
    #[snafu(display(
        "docker container {container_id} exited prematurely with status {exit_status}"
    ))]
    ContainerExitedPrematurely {
        container_id: String,
        exit_status: i64,
    },
    #[snafu(display("failed to create status directory"))]
    CreateStatusDir { source: std::io::Error },
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
