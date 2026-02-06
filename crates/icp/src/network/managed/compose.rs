//! Docker Compose network management.
//!
//! This module handles starting, stopping, and monitoring Docker Compose-based networks.
//! Compose networks allow running multi-container setups like Bitcoin regtest alongside
//! the IC network launcher.

use std::time::Duration;

use async_dropper::{AsyncDrop, AsyncDropper};
use async_trait::async_trait;
use bollard::{
    Docker,
    query_parameters::{InspectContainerOptions, ListContainersOptions},
};
use snafu::prelude::*;
use tokio::select;

use crate::{
    network::{ManagedComposeConfig, config::ChildLocator, managed::launcher::NetworkInstance},
    prelude::*,
};

use super::{docker::connect_docker, launcher::wait_for_launcher_status};

/// Check if a Docker Compose project is running by checking if its containers exist.
pub async fn is_compose_running(socket: &str, project_name: &str) -> bool {
    let Ok(docker) = connect_docker(socket) else {
        return false;
    };

    // List containers with the compose project label
    let filters: std::collections::HashMap<String, Vec<String>> = [(
        "label".to_string(),
        vec![format!("com.docker.compose.project={project_name}")],
    )]
    .into();

    match docker
        .list_containers(Some(ListContainersOptions {
            filters: Some(filters),
            ..Default::default()
        }))
        .await
    {
        Ok(containers) => {
            // Check if any containers are running
            containers.iter().any(|c| {
                c.state
                    .as_ref()
                    .is_some_and(|s| s.to_string().to_lowercase() == "running")
            })
        }
        Err(_) => false,
    }
}

#[derive(Debug, Snafu)]
pub enum ComposeError {
    #[snafu(display("docker compose file not found: {path}"))]
    ComposeFileNotFound { path: PathBuf },

    #[snafu(display("failed to start compose services: {message}"))]
    StartServices { message: String },

    #[snafu(display("failed to stop compose services: {message}"))]
    StopServices { message: String },

    #[snafu(display("gateway service '{service}' not found in compose project"))]
    GatewayServiceNotFound { service: String },

    #[snafu(display("timed out waiting for gateway status after {seconds} seconds"))]
    StatusTimeout { seconds: u64 },

    #[snafu(display(
        "gateway service '{service}' exited unexpectedly. \
         Check logs with: docker compose -f {compose_file} -p {project_name} logs {service}"
    ))]
    GatewayServiceExited {
        service: String,
        compose_file: String,
        project_name: String,
    },

    #[snafu(display("failed to read status from container: {message}"))]
    ReadStatus { message: String },

    #[snafu(transparent)]
    ConnectDocker {
        source: super::docker::ConnectDockerError,
    },

    #[snafu(transparent)]
    WaitForLauncherStatus {
        source: super::launcher::WaitForLauncherStatusError,
    },

    #[snafu(transparent)]
    WatchStatusDir {
        source: super::launcher::WaitForFileError,
    },

    #[snafu(display("failed to parse root key {key}"))]
    ParseRootKey {
        key: String,
        source: hex::FromHexError,
    },

    #[snafu(display("failed to create status directory"))]
    CreateStatusDir { source: std::io::Error },

    #[snafu(display("failed to inspect container {container_id}"))]
    InspectContainer {
        source: bollard::errors::Error,
        container_id: String,
    },

    #[snafu(display("required field {field} missing from docker API"))]
    RequiredFieldMissing { field: String },
}

/// Manages a Docker Compose network lifecycle.
pub struct ComposeNetwork {
    project_name: String,
    compose_file: PathBuf,
    gateway_service: String,
    environment: Vec<String>,
}

impl ComposeNetwork {
    pub fn new(network_name: &str, config: &ManagedComposeConfig, project_root: &Path) -> Self {
        Self {
            project_name: format!("icp-{network_name}"),
            compose_file: project_root.join(&config.file),
            gateway_service: config.gateway_service.clone(),
            environment: config.environment.clone(),
        }
    }

    /// Start the Docker Compose services and wait for the gateway to be ready.
    pub async fn start(
        &self,
    ) -> Result<
        (
            AsyncDropper<ComposeDropGuard>,
            NetworkInstance,
            ChildLocator,
        ),
        ComposeError,
    > {
        // Verify compose file exists
        ensure!(
            self.compose_file.exists(),
            ComposeFileNotFoundSnafu {
                path: &self.compose_file
            }
        );

        // Get Docker socket
        let socket = get_docker_socket()?;

        // Create a temporary directory for the status file on the host
        let host_status_tmpdir =
            camino_tempfile::Utf8TempDir::new().context(CreateStatusDirSnafu)?;
        let host_status_dir = host_status_tmpdir.path().to_path_buf();

        // Build environment variables for compose
        let mut env_vars: Vec<(&str, &str)> = self
            .environment
            .iter()
            .filter_map(|e| e.split_once('='))
            .collect();

        // Add the status directory as an environment variable so compose can mount it
        let status_dir_str = host_status_dir.to_string();
        env_vars.push(("ICP_STATUS_DIR", &status_dir_str));

        // Set up the file watcher before starting compose
        let watcher = wait_for_launcher_status(&host_status_dir)?;

        // Start compose services
        let mut cmd = tokio::process::Command::new("docker");
        cmd.args([
            "compose",
            "-f",
            self.compose_file.as_str(),
            "-p",
            &self.project_name,
            "up",
            "-d",
            "--wait",
        ]);

        // Add environment variables
        for (key, value) in &env_vars {
            cmd.env(key, value);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ComposeError::StartServices {
                message: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(ComposeError::StartServices {
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        eprintln!("Started compose project '{}'", self.project_name);

        // Create drop guard
        let guard = AsyncDropper::new(ComposeDropGuard {
            project_name: Some(self.project_name.clone()),
            compose_file: Some(self.compose_file.clone()),
        });

        // Connect to Docker for container monitoring and port retrieval
        let docker = connect_docker(&socket)?;
        let container_name = format!("{}-{}-1", self.project_name, self.gateway_service);

        // Wait for gateway status with timeout and container monitoring
        let timeout_seconds = 120u64;
        let launcher_status = select! {
            status = watcher => status?,
            _ = tokio::time::sleep(Duration::from_secs(timeout_seconds)) => {
                return StatusTimeoutSnafu { seconds: timeout_seconds }.fail();
            }
            _ = monitor_gateway_exit(&docker, &container_name) => {
                return GatewayServiceExitedSnafu {
                    service: &self.gateway_service,
                    compose_file: self.compose_file.as_str(),
                    project_name: &self.project_name,
                }.fail();
            }
        };

        // Get the gateway container's mapped port
        let gateway_port = self
            .get_gateway_port(&docker, launcher_status.gateway_port)
            .await?;

        let locator = ChildLocator::Compose {
            project_name: self.project_name.clone(),
            compose_file: self.compose_file.to_string(),
            socket,
        };

        Ok((
            guard,
            NetworkInstance {
                gateway_port,
                root_key: hex::decode(&launcher_status.root_key).context(ParseRootKeySnafu {
                    key: &launcher_status.root_key,
                })?,
                pocketic_config_port: launcher_status.config_port,
                pocketic_instance_id: launcher_status.instance_id,
            },
            locator,
        ))
    }

    /// Get the host port mapped to the gateway container's port.
    async fn get_gateway_port(
        &self,
        docker: &Docker,
        container_port: u16,
    ) -> Result<u16, ComposeError> {
        let container_name = format!("{}-{}-1", self.project_name, self.gateway_service);

        let info = docker
            .inspect_container(&container_name, None::<InspectContainerOptions>)
            .await
            .context(InspectContainerSnafu {
                container_id: &container_name,
            })?;

        let port_bindings = info
            .network_settings
            .ok_or(ComposeError::RequiredFieldMissing {
                field: "NetworkSettings".to_string(),
            })?
            .ports
            .ok_or(ComposeError::RequiredFieldMissing {
                field: "NetworkSettings.Ports".to_string(),
            })?;

        let port_key = format!("{container_port}/tcp");
        let binding = port_bindings
            .get(&port_key)
            .and_then(|b| b.as_ref())
            .and_then(|b| b.first())
            .ok_or(ComposeError::RequiredFieldMissing {
                field: format!("Port binding for {port_key}"),
            })?;

        let host_port_str =
            binding
                .host_port
                .as_ref()
                .ok_or(ComposeError::RequiredFieldMissing {
                    field: "HostPort".to_string(),
                })?;

        host_port_str
            .parse::<u16>()
            .map_err(|_| ComposeError::RequiredFieldMissing {
                field: format!("Valid port number (got {host_port_str})"),
            })
    }
}

/// Resolves when the given container is no longer running.
/// Used to detect early gateway crashes during startup.
async fn monitor_gateway_exit(docker: &Docker, container_name: &str) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let running = match docker
            .inspect_container(container_name, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => info.state.and_then(|s| s.running).unwrap_or(false),
            Err(_) => false,
        };
        if !running {
            return;
        }
    }
}

/// Stop a Docker Compose project.
pub async fn stop_compose(compose_file: &str, project_name: &str) -> Result<(), ComposeError> {
    let output = tokio::process::Command::new("docker")
        .args(["compose", "-f", compose_file, "-p", project_name, "down"])
        .output()
        .await
        .map_err(|e| ComposeError::StopServices {
            message: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(ComposeError::StopServices {
            message: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    eprintln!("Stopped compose project '{project_name}'");
    Ok(())
}

/// Get logs from a Docker Compose project.
pub async fn get_compose_logs(
    compose_file: &str,
    project_name: &str,
    service: Option<&str>,
    follow: bool,
    tail: Option<u32>,
) -> Result<tokio::process::Child, ComposeError> {
    let mut cmd = tokio::process::Command::new("docker");
    cmd.args(["compose", "-f", compose_file, "-p", project_name, "logs"]);

    if follow {
        cmd.arg("-f");
    }

    if let Some(n) = tail {
        cmd.args(["--tail", &n.to_string()]);
    }

    if let Some(svc) = service {
        cmd.arg(svc);
    }

    cmd.stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| ComposeError::StartServices {
            message: format!("Failed to get logs: {e}"),
        })
}

/// Get the status of services in a Docker Compose project.
pub async fn get_compose_status(
    compose_file: &str,
    project_name: &str,
) -> Result<Vec<ServiceStatus>, ComposeError> {
    let output = tokio::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            compose_file,
            "-p",
            project_name,
            "ps",
            "--format",
            "json",
        ])
        .output()
        .await
        .map_err(|e| ComposeError::StartServices {
            message: format!("Failed to get status: {e}"),
        })?;

    if !output.status.success() {
        return Err(ComposeError::StartServices {
            message: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    // Parse JSON output (docker compose ps --format json outputs one JSON object per line)
    let stdout = String::from_utf8_lossy(&output.stdout);
    let services: Vec<ServiceStatus> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    Ok(services)
}

/// Status of a single service in a Docker Compose project.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ServiceStatus {
    #[serde(alias = "Name")]
    pub name: String,
    #[serde(alias = "Service")]
    pub service: String,
    #[serde(alias = "State")]
    pub state: String,
    #[serde(alias = "Health", default)]
    pub health: String,
    #[serde(alias = "Image")]
    pub image: String,
}

/// Get the Docker socket path.
fn get_docker_socket() -> Result<String, ComposeError> {
    if let Ok(socket) = std::env::var("DOCKER_HOST") {
        return Ok(socket);
    }

    #[cfg(unix)]
    {
        let default_sock = "/var/run/docker.sock";
        if Path::new(default_sock).exists() {
            return Ok(default_sock.to_string());
        }

        // Try to get socket from docker context
        let output = std::process::Command::new("docker")
            .args([
                "context",
                "inspect",
                "--format",
                "{{.Endpoints.docker.Host}}",
            ])
            .output()
            .map_err(|e| ComposeError::StartServices {
                message: format!("Failed to get docker context: {e}"),
            })?;

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }

        Ok(default_sock.to_string())
    }

    #[cfg(windows)]
    {
        Ok(r"\\.\pipe\docker_engine".to_string())
    }
}

/// Drop guard for Docker Compose projects.
#[derive(Default)]
pub struct ComposeDropGuard {
    project_name: Option<String>,
    compose_file: Option<PathBuf>,
}

impl ComposeDropGuard {
    /// Disarm the guard so it won't stop the compose project on drop.
    pub fn defuse(&mut self) {
        self.project_name = None;
        self.compose_file = None;
    }

    /// Stop the compose project.
    pub async fn stop(&mut self) -> Result<(), ComposeError> {
        if let (Some(project_name), Some(compose_file)) =
            (self.project_name.take(), self.compose_file.take())
        {
            stop_compose(compose_file.as_str(), &project_name).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl AsyncDrop for ComposeDropGuard {
    async fn async_drop(&mut self) {
        let _ = self.stop().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::ManagedComposeConfig;

    #[test]
    fn compose_network_generates_correct_project_name() {
        let config = ManagedComposeConfig {
            file: PathBuf::from("docker-compose.yml"),
            gateway_service: "icp-network".to_string(),
            environment: vec![],
        };

        let network = ComposeNetwork::new("local-bitcoin", &config, &PathBuf::from("/project"));

        assert_eq!(network.project_name, "icp-local-bitcoin");
        assert_eq!(
            network.compose_file,
            PathBuf::from("/project/docker-compose.yml")
        );
        assert_eq!(network.gateway_service, "icp-network");
    }

    #[test]
    fn compose_network_joins_relative_compose_file_with_project_root() {
        let config = ManagedComposeConfig {
            file: PathBuf::from("infra/docker-compose.bitcoin.yml"),
            gateway_service: "gateway".to_string(),
            environment: vec!["FOO=bar".to_string()],
        };

        let network =
            ComposeNetwork::new("btc-test", &config, &PathBuf::from("/home/user/myproject"));

        assert_eq!(
            network.compose_file,
            PathBuf::from("/home/user/myproject/infra/docker-compose.bitcoin.yml")
        );
    }

    #[test]
    fn compose_drop_guard_defuse_clears_fields() {
        let mut guard = ComposeDropGuard {
            project_name: Some("test-project".to_string()),
            compose_file: Some(PathBuf::from("/path/to/compose.yml")),
        };

        guard.defuse();

        assert!(guard.project_name.is_none());
        assert!(guard.compose_file.is_none());
    }

    #[test]
    fn service_status_deserializes_from_docker_compose_output() {
        let json = r#"{"Name":"icp-local-1","Service":"icp-network","State":"running","Health":"","Image":"ghcr.io/dfinity/icp-cli-network-launcher:latest"}"#;

        let status: ServiceStatus = serde_json::from_str(json).unwrap();

        assert_eq!(status.name, "icp-local-1");
        assert_eq!(status.service, "icp-network");
        assert_eq!(status.state, "running");
        assert_eq!(
            status.image,
            "ghcr.io/dfinity/icp-cli-network-launcher:latest"
        );
    }
}
