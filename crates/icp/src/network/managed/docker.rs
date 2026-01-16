use std::collections::HashMap;

use async_dropper::{AsyncDrop, AsyncDropper};
use async_trait::async_trait;
use bollard::{
    Docker,
    errors::Error as BollardError,
    query_parameters::{
        CreateContainerOptions, CreateImageOptions, InspectContainerOptions,
        RemoveContainerOptions, StartContainerOptions, StopContainerOptions, WaitContainerOptions,
    },
    secret::{ContainerCreateBody, HostConfig, Mount, MountTypeEnum, PortBinding},
};
use camino_tempfile::Utf8TempDir;
use futures::{StreamExt, TryStreamExt};
use itertools::Itertools;
use snafu::ResultExt;
use snafu::{OptionExt, Snafu};
use tokio::select;

use crate::network::{
    ManagedImageConfig,
    config::ChildLocator,
    managed::launcher::{NetworkInstance, wait_for_launcher_status},
};
use crate::prelude::*;

pub async fn spawn_docker_launcher(
    image_config: &ManagedImageConfig,
) -> Result<(AsyncDropper<DockerDropGuard>, NetworkInstance, ChildLocator), DockerLauncherError> {
    let ManagedImageConfig {
        image,
        port_mapping,
        rm_on_exit,
        args,
        entrypoint,
        environment,
        volumes,
        platform,
        user,
        shm_size,
        status_dir,
        mounts,
    } = image_config;
    let platform = if let Some(p) = platform {
        p.clone()
    } else if cfg!(target_arch = "aarch64") {
        "linux/arm64".to_string()
    } else {
        "linux/amd64".to_string()
    };
    let host_status_dir = Utf8TempDir::new().context(CreateStatusDirSnafu)?;
    let socket = match std::env::var("DOCKER_HOST").ok() {
        Some(sock) => sock,
        #[cfg(unix)]
        None => {
            let default_sock = "/var/run/docker.sock".to_string();
            if Path::new(&default_sock).exists() {
                default_sock
            } else {
                let command_res = std::process::Command::new("docker")
                    .args([
                        "context",
                        "inspect",
                        "--format",
                        "{{.Endpoints.docker.Host}}",
                    ])
                    .output()
                    .context(NoGlobalSocketAndShellSnafu)?;
                if !command_res.status.success() {
                    return NoGlobalSocketAndCommandSnafu {
                        stderr: String::from_utf8_lossy(&command_res.stderr).to_string(),
                    }
                    .fail();
                }
                str::from_utf8(&command_res.stdout)
                    .context(NoGlobalSocketAndParseCommandSnafu {
                        display: String::from_utf8_lossy(&command_res.stdout),
                    })?
                    .trim()
                    .to_string()
            }
        }
        #[cfg(windows)]
        None => r"\\.\pipe\docker_engine".to_string(),
    };
    let docker = Docker::connect_with_local(&socket, 120, bollard::API_DEFAULT_VERSION)
        .context(ConnectDockerSnafu { socket: &socket })?;
    let portmap: HashMap<_, _> = port_mapping
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
    let mounts = mounts
        .iter()
        .map(|m| {
            let (host, rest) = m.split_once(':').context(ParseMountSnafu { mount: m })?;
            let host = dunce::canonicalize(host).context(ProcessMountSourceSnafu { path: host })?;
            let host = PathBuf::try_from(host.clone()).context(BadPathSnafu)?;
            let (target, flags) = match rest.split_once(':') {
                Some((t, f)) => (t, Some(f)),
                None => (rest, None),
            };
            let read_only = flags
                .map(|f| match f {
                    "ro" => Ok(true),
                    "rw" => Ok(false),
                    _ => Err(UnknownFlagsSnafu { flags: f }.build()),
                })
                .transpose()?;
            Ok::<_, DockerLauncherError>(Mount {
                target: Some(target.to_string()),
                source: Some(host.to_string()),
                typ: Some(MountTypeEnum::BIND),
                read_only,
                ..<_>::default()
            })
        })
        .chain([Ok(Mount {
            target: Some(status_dir.to_string()),
            source: Some(host_status_dir.path().to_string()),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..<_>::default()
        })])
        .try_collect()?;
    let image_query = docker.inspect_image(image).await;
    match image_query {
        Ok(_) => {}
        Err(BollardError::DockerResponseServerError {
            status_code: 404, ..
        }) => {
            eprintln!("Pulling image {image}");
            docker
                .create_image(
                    Some(CreateImageOptions {
                        from_image: Some(image.clone()),
                        platform: platform.clone(),
                        ..<_>::default()
                    }),
                    None,
                    None,
                )
                .map(|r| r.map(|_| ()))
                .try_collect::<()>()
                .await
                .context(PullImageSnafu { image })?;
        }
        Err(e) => return Err(e).context(QueryImageSnafu { image }),
    };
    let container_resp = docker
        .create_container(
            Some(CreateContainerOptions {
                platform: platform.clone(),
                ..<_>::default()
            }),
            ContainerCreateBody {
                image: Some(image.to_string()),
                cmd: Some(args.to_vec()),
                entrypoint: entrypoint.clone(),
                env: Some(
                    environment
                        .iter()
                        .cloned()
                        .chain(["ICP_CLI_NETWORK_LAUNCHER_INTERFACE_VERSION=1.0.0".to_string()])
                        .collect(),
                ),
                user: user.clone(),
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                host_config: Some(HostConfig {
                    port_bindings: Some(portmap),
                    mounts: Some(mounts),
                    binds: Some(volumes.to_vec()),
                    shm_size: *shm_size,
                    ..<_>::default()
                }),
                ..<_>::default()
            },
        )
        .await
        .context(CreateContainerSnafu { image_name: image })?;
    let container_id = container_resp.id;
    eprintln!("Created container {}", &container_id[..12]);
    let guard = AsyncDropper::new(DockerDropGuard {
        container_id: Some(container_id),
        docker: Some(docker),
        rm_on_drop: *rm_on_exit,
    });
    let container_id = guard.container_id.as_ref().unwrap();
    let docker = guard.docker.as_ref().unwrap();
    let watcher = wait_for_launcher_status(host_status_dir.path())?;
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
    let locator = ChildLocator::Container {
        id: container_id.clone(),
        socket,
        rm_on_exit: *rm_on_exit,
    };
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
        locator,
    ))
}

pub async fn stop_docker_launcher(
    socket: &str,
    container_id: &str,
    rm_on_exit: bool,
) -> Result<(), StopContainerError> {
    let docker = Docker::connect_with_local(socket, 120, bollard::API_DEFAULT_VERSION)
        .context(ConnectSnafu { socket })?;
    stop(&docker, container_id, rm_on_exit).await
}

async fn stop(
    docker: &Docker,
    container_id: &str,
    rm_on_exit: bool,
) -> Result<(), StopContainerError> {
    docker
        .stop_container(container_id, None::<StopContainerOptions>)
        .await
        .context(StopSnafu { container_id })?;
    if rm_on_exit {
        docker
            .remove_container(container_id, None::<RemoveContainerOptions>)
            .await
            .context(RemoveSnafu { container_id })?;
    }
    Ok(())
}

#[derive(Snafu, Debug)]
pub enum StopContainerError {
    #[snafu(display("failed to connect to docker daemon at {socket} (is it running?)"))]
    Connect {
        source: bollard::errors::Error,
        socket: String,
    },
    #[snafu(display("failed to stop docker container {container_id}"))]
    Stop {
        source: bollard::errors::Error,
        container_id: String,
    },
    #[snafu(display("failed to remove docker container {container_id}"))]
    Remove {
        source: bollard::errors::Error,
        container_id: String,
    },
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
    #[snafu(display("failed to locate or pull docker image {image}"))]
    PullImage {
        source: bollard::errors::Error,
        image: String,
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
    #[snafu(display(
        "failed to parse mount {mount}, must be in format <host_path>:<container_path>[:<options>]"
    ))]
    ParseMount { mount: String },
    #[snafu(display("failed to convert path {path} to absolute path"))]
    ProcessMountSource {
        source: std::io::Error,
        path: String,
    },
    #[snafu(display("failed to process path as UTF-8"))]
    BadPath { source: camino::FromPathBufError },
    #[snafu(display("unknown mount flags {flags}, expected 'ro' or 'rw'"))]
    UnknownFlags { flags: String },
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
    #[snafu(display("failed to query docker image {image}"))]
    QueryImage {
        source: bollard::errors::Error,
        image: String,
    },
    #[snafu(display("image {image} not found (try running `docker pull {image}`)"))]
    NoSuchImage { image: String },
    #[snafu(display(
        "docker socket was not at /var/run/docker.sock; DOCKER_HOST is not set; and error shelling out to `docker context`"
    ))]
    NoGlobalSocketAndShellError { source: std::io::Error },
    #[snafu(display(
        "docker socket was not at /var/run/docker.sock; DOCKER_HOST is not set; and `docker context` errored with: {stderr}"
    ))]
    NoGlobalSocketAndCommandError { stderr: String },
    #[snafu(display(
        "docker socket was not at /var/run/docker.sock; DOCKER_HOST is not set; and error parsing `docker context` output as UTF-8: {display}"
    ))]
    NoGlobalSocketAndParseCommandError {
        source: std::str::Utf8Error,
        display: String,
    },
}

#[derive(Default)]
pub struct DockerDropGuard {
    docker: Option<Docker>,
    container_id: Option<String>,
    rm_on_drop: bool,
}

impl DockerDropGuard {
    pub fn defuse(&mut self) {
        self.docker = None;
        self.container_id = None;
    }
    pub async fn stop(&mut self) -> Result<(), StopContainerError> {
        if let Some(docker) = &self.docker.take() {
            let container_id = self.container_id.take().unwrap();
            stop(docker, &container_id, self.rm_on_drop).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl AsyncDrop for DockerDropGuard {
    async fn async_drop(&mut self) {
        _ = self.stop().await;
    }
}
