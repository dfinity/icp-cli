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
use wslpath2::Conversion;

use crate::network::{
    ManagedImageConfig, config::ChildLocator, managed::launcher::NetworkInstance,
};
use crate::prelude::*;

use super::launcher::wait_for_launcher_status;

/// Parsed and validated options for spawning a Docker container.
/// Created from `ManagedImageConfig` via `TryFrom`.
pub struct ManagedImageOptions {
    pub image: String,
    pub port_bindings: HashMap<String, Option<Vec<PortBinding>>>,
    pub rm_on_exit: bool,
    pub args: Vec<String>,
    pub entrypoint: Option<Vec<String>>,
    pub environment: Vec<String>,
    /// Volumes with paths already converted for WSL2 if needed.
    pub volumes: Vec<String>,
    pub platform: String,
    pub user: Option<String>,
    pub shm_size: Option<i64>,
    /// Container path where the status directory will be mounted.
    pub status_dir: String,
    /// Parsed mounts (excluding the status directory mount, which is added at runtime).
    pub mounts: Vec<Mount>,
}

impl TryFrom<&ManagedImageConfig> for ManagedImageOptions {
    type Error = ManagedImageConversionError;

    fn try_from(config: &ManagedImageConfig) -> Result<Self, Self::Error> {
        let wsl2_distro = std::env::var("ICP_CLI_DOCKER_WSL2_DISTRO").ok();
        let wsl2_distro = wsl2_distro.as_deref();
        let wsl2_convert = cfg!(windows) && wsl2_distro.is_some();

        let platform = config.platform.clone().unwrap_or_else(|| {
            if cfg!(target_arch = "aarch64") {
                "linux/arm64".to_string()
            } else {
                "linux/amd64".to_string()
            }
        });

        let port_bindings = config
            .port_mapping
            .iter()
            .map(|mapping| {
                let (host, container_port) =
                    mapping.rsplit_once(':').context(ParsePortmapSnafu {
                        port_mapping: mapping,
                    })?;
                let (host_ip, host_port) = if let Some((ip, port)) = host.rsplit_once(':') {
                    (ip.to_string(), port.to_string())
                } else {
                    ("127.0.0.1".to_string(), host.to_string())
                };
                Ok::<_, ManagedImageConversionError>((
                    format!("{container_port}/tcp"),
                    Some(vec![PortBinding {
                        host_ip: Some(host_ip),
                        host_port: Some(host_port),
                    }]),
                ))
            })
            .try_collect()?;

        let mounts = config
            .mounts
            .iter()
            .map(|m| {
                let (host, rest) = m.split_once(':').context(ParseMountSnafu { mount: m })?;
                let host =
                    dunce::canonicalize(host).context(ProcessMountSourceSnafu { path: host })?;
                let host = PathBuf::try_from(host.clone()).context(BadPathSnafu)?;
                let host_param = convert_path(wsl2_convert, wsl2_distro, &host)?;
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
                Ok::<_, ManagedImageConversionError>(Mount {
                    target: Some(target.to_string()),
                    source: Some(host_param),
                    typ: Some(MountTypeEnum::BIND),
                    read_only,
                    ..<_>::default()
                })
            })
            .try_collect()?;

        let volumes = config
            .volumes
            .iter()
            .map(|v| convert_volume(wsl2_convert, wsl2_distro, v))
            .try_collect()?;

        Ok(ManagedImageOptions {
            image: config.image.clone(),
            port_bindings,
            rm_on_exit: config.rm_on_exit,
            args: config.args.clone(),
            entrypoint: config.entrypoint.clone(),
            environment: config.environment.clone(),
            volumes,
            platform,
            user: config.user.clone(),
            shm_size: config.shm_size,
            status_dir: config.status_dir.clone(),
            mounts,
        })
    }
}

#[derive(Debug, Snafu)]
pub enum ManagedImageConversionError {
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
    #[snafu(display("failed to convert path {path} to WSL2: {msg}"))]
    WslPathConvert { msg: String, path: PathBuf },
}

pub async fn spawn_docker_launcher(
    options: &ManagedImageOptions,
) -> Result<
    (
        AsyncDropper<DockerDropGuard>,
        NetworkInstance,
        ChildLocator,
        bool,
    ),
    DockerLauncherError,
> {
    let ManagedImageOptions {
        image,
        port_bindings,
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
    } = options;

    // Create status tmpdir and convert path for WSL2 if needed
    let wsl2_distro = std::env::var("ICP_CLI_DOCKER_WSL2_DISTRO").ok();
    let wsl2_distro = wsl2_distro.as_deref();
    let wsl2_convert = cfg!(windows) && wsl2_distro.is_some();
    let host_status_tmpdir = Utf8TempDir::new().context(CreateStatusDirSnafu)?;
    let host_status_dir = host_status_tmpdir.path();
    let host_status_dir_param = convert_path(wsl2_convert, wsl2_distro, host_status_tmpdir.path())
        .map_err(|e| match e {
            ManagedImageConversionError::WslPathConvert { msg, path } => {
                WslStatusDirPathConvertSnafu { msg, path }.build()
            }
            // Other variants can't occur from convert_path
            _ => unreachable!(),
        })?;

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

    let docker = connect_docker(&socket)?;

    // Add status dir mount to the mounts
    let all_mounts: Vec<Mount> = mounts
        .iter()
        .cloned()
        .chain([Mount {
            target: Some(status_dir.to_string()),
            source: Some(host_status_dir_param),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..<_>::default()
        }])
        .collect();
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
                    port_bindings: Some(port_bindings.clone()),
                    mounts: Some(all_mounts),
                    binds: Some(volumes.clone()),
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
    let watcher = wait_for_launcher_status(host_status_dir)?;
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
    let gateway_port_was_fixed = port_bindings
        .get(&format!("{container_gateway_port}/tcp"))
        .is_some_and(|p| {
            p.as_ref()
                .unwrap()
                .iter()
                .any(|m| m.host_port.as_ref().unwrap() != "0")
        });
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
        gateway_port_was_fixed,
    ))
}

/// Connect to the Docker daemon at the given socket.
pub fn connect_docker(socket: &str) -> Result<Docker, ConnectDockerError> {
    if socket.starts_with("tcp://") || socket.starts_with("http://") {
        let http_addr = socket.replace("tcp://", "http://");
        Docker::connect_with_http(&http_addr, 120, bollard::API_DEFAULT_VERSION)
    } else {
        Docker::connect_with_local(socket, 120, bollard::API_DEFAULT_VERSION)
    }
    .context(ConnectDockerSnafu { socket })
}

#[derive(Snafu, Debug)]
#[snafu(display("failed to connect to docker daemon at {socket} (is it running?)"))]
pub struct ConnectDockerError {
    source: bollard::errors::Error,
    socket: String,
}

/// Check if a container is running.
pub async fn is_container_running(socket: &str, container_id: &str) -> bool {
    let Ok(docker) = connect_docker(socket) else {
        return false;
    };
    match docker
        .inspect_container(container_id, None::<InspectContainerOptions>)
        .await
    {
        Ok(info) => info.state.and_then(|s| s.running).unwrap_or(false),
        Err(_) => false,
    }
}

pub async fn stop_docker_launcher(
    socket: &str,
    container_id: &str,
    rm_on_exit: bool,
) -> Result<(), StopContainerError> {
    let docker = connect_docker(socket)?;
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
    #[snafu(transparent)]
    StopConnect { source: ConnectDockerError },
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

fn convert_path(
    convert: bool,
    distro: Option<&str>,
    path: &Path,
) -> Result<String, ManagedImageConversionError> {
    if convert {
        wslpath2::convert(path.as_str(), distro, Conversion::WindowsToWsl, true).map_err(|e| {
            WslPathConvertSnafu {
                msg: e.to_string(),
                path: path.to_path_buf(),
            }
            .build()
        })
    } else {
        Ok(path.to_string())
    }
}

fn convert_volume(
    convert: bool,
    distro: Option<&str>,
    volume: &str,
) -> Result<String, ManagedImageConversionError> {
    // docker's actual parsing logic, clunky as it is
    let (host, rest) = if volume.chars().next().unwrap().is_ascii_alphabetic()
        && volume.chars().nth(1).unwrap() == ':'
    {
        let split_point = volume[2..]
            .find(':')
            .map(|idx| idx + 2)
            .context(ParseMountSnafu { mount: volume })?;
        (&volume[..split_point], &volume[split_point + 1..])
    } else {
        volume
            .split_once(':')
            .context(ParseMountSnafu { mount: volume })?
    };
    let host_param = if host.contains(&['/', '\\'][..]) {
        let host_path =
            dunce::canonicalize(host).context(ProcessMountSourceSnafu { path: host })?;
        let host_path = PathBuf::try_from(host_path.clone()).context(BadPathSnafu)?;
        convert_path(convert, distro, &host_path)?
    } else {
        host.to_string()
    };
    Ok(format!("{host_param}:{rest}"))
}

#[derive(Debug, Snafu)]
pub enum DockerLauncherError {
    #[snafu(transparent)]
    ConnectDocker { source: ConnectDockerError },
    #[snafu(transparent)]
    ImageConversion { source: ManagedImageConversionError },
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
    #[snafu(display("failed to convert status dir path {path} to WSL2: {msg}"))]
    WslStatusDirPathConvert { msg: String, path: PathBuf },
    #[snafu(display("failed to create temporary directory in WSL2"))]
    WslCreateTmpDirError { source: std::io::Error },
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
