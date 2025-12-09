use camino_tempfile::Utf8TempDir;
use candid::Principal;
use notify::Watcher;
use pocket_ic::common::rest::{
    AutoProgressConfig, CreateHttpGatewayResponse, CreateInstanceResponse, HttpGatewayBackend,
    HttpGatewayConfig, HttpGatewayInfo, IcpConfig, IcpConfigFlag, IcpFeatures, IcpFeaturesConfig,
    InstanceConfig, InstanceId, RawTime, SubnetConfigSet, Topology,
};
use reqwest::Url;
use serde::Deserialize;
use snafu::prelude::*;
use std::{io::ErrorKind, process::Stdio};
use time::OffsetDateTime;
use tokio::process::Child;

use crate::{network::Port, prelude::*};

pub fn default_instance_config(state_dir: &Path) -> InstanceConfig {
    InstanceConfig {
        // State directory
        state_dir: Some(state_dir.to_path_buf().into()),

        // Replica logging level
        log_level: Some("ERROR".to_string()),

        // Special features
        icp_features: Some(IcpFeatures {
            // Enable with default feature configuration
            icp_token: Some(IcpFeaturesConfig::DefaultConfig),

            // Same as above
            cycles_token: Some(IcpFeaturesConfig::DefaultConfig),

            // Same as above
            cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),

            // Same as above
            registry: Some(IcpFeaturesConfig::DefaultConfig),

            // Same as above
            ii: Some(IcpFeaturesConfig::DefaultConfig),

            // The rest of the features are disabled for now
            nns_governance: None,
            sns: None,
            nns_ui: None,
            // do not use ..default() here so we notice if new features are available
        }),

        subnet_config_set: (SubnetConfigSet {
            application: 1,

            // The rest of the subnets are disabled by default
            ..Default::default()
        })
        .into(),

        icp_config: Some(IcpConfig {
            // Required to enable environment variables
            beta_features: Some(IcpConfigFlag::Enabled),
            ..Default::default()
        }),

        ..Default::default()
    }
}

pub struct PocketIcInstance {
    pub admin: PocketIcAdminInterface,
    pub gateway_port: u16,
    pub instance_id: InstanceId,
    pub effective_canister_id: Principal,
    pub root_key: String,
}

pub async fn spawn_network_launcher(
    network_launcher_path: &Path,
    stdout_file: &Path,
    stderr_file: &Path,
    background: bool,
    port: &Port,
    state_dir: &Path,
) -> (Child, PocketIcInstance) {
    let mut cmd = tokio::process::Command::new(network_launcher_path);
    cmd.args([
        "--interface-version",
        "1.0.0",
        "--state-dir",
        state_dir.as_str(),
    ]);
    if let Port::Fixed(port) = port {
        cmd.args(["--gateway-port", &port.to_string()]);
    }
    let status_dir = Utf8TempDir::new().unwrap();
    cmd.args(["--status-dir", status_dir.path().as_str()]);
    if background {
        eprintln!("For background mode, PocketIC output will be redirected:");
        eprintln!("  stdout: {}", stdout_file);
        eprintln!("  stderr: {}", stderr_file);
        let stdout = std::fs::File::create(stdout_file).expect("Failed to create stdout file.");
        let stderr = std::fs::File::create(stderr_file).expect("Failed to create stderr file.");
        cmd.stdout(Stdio::from(stdout));
        cmd.stderr(Stdio::from(stderr));
    } else {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }
    let watcher = wait_for_single_line_file(&status_dir.path().join("status.json")).unwrap();
    let child = cmd.spawn().expect("Could not start network launcher.");
    let status_content = watcher
        .await
        .expect("Failed to read network launcher status.");
    let launcher_status: LauncherStatus =
        serde_json::from_str(&status_content).expect("Failed to parse network launcher status.");
    assert_eq!(
        launcher_status.v, "1",
        "unexpected network launcher status version"
    );
    (
        child,
        PocketIcInstance {
            admin: PocketIcAdminInterface::new(
                Url::parse(&format!("http://localhost:{}", launcher_status.config_port)).unwrap(),
            ),
            gateway_port: launcher_status.gateway_port,
            instance_id: launcher_status.instance_id,
            effective_canister_id: launcher_status.default_effective_canister_id,
            root_key: launcher_status.root_key,
        },
    )
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
fn wait_for_single_line_file(
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

#[derive(Deserialize)]
struct LauncherStatus {
    v: String,
    instance_id: usize,
    config_port: u16,
    gateway_port: u16,
    root_key: String,
    default_effective_canister_id: Principal,
}

pub struct PocketIcAdminInterface {
    client: reqwest::Client,
    pub base_url: Url,
}

impl PocketIcAdminInterface {
    pub fn new(base_url: Url) -> Self {
        let client = reqwest::Client::new();
        Self { client, base_url }
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.post(self.base_url.join(path).unwrap())
    }

    pub async fn create_instance_with_config(
        &self,
        inst_cfg: InstanceConfig,
    ) -> Result<(InstanceId, Topology), CreateInstanceError> {
        // Perform request
        let resp = self.post("/instances").json(&inst_cfg).send().await?;

        // Check for replica errors
        let resp = resp
            .error_for_status()?
            .json::<CreateInstanceResponse>()
            .await?;

        match resp {
            CreateInstanceResponse::Created {
                instance_id,
                topology,
                ..
            } => Ok((instance_id, topology)),

            CreateInstanceResponse::Error { message } => {
                Err(CreateInstanceError::Create { message })
            }
        }
    }

    pub(crate) async fn set_time(&self, instance_id: InstanceId) -> Result<(), reqwest::Error> {
        self.post(&format!("/instances/{instance_id}/update/set_time"))
            .json(&RawTime {
                nanos_since_epoch: OffsetDateTime::now_utc()
                    .unix_timestamp_nanos()
                    .try_into()
                    .unwrap(),
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub(crate) async fn auto_progress(
        &self,
        instance_id: InstanceId,
        artificial_delay: i32,
    ) -> Result<(), reqwest::Error> {
        self.post(&format!("/instances/{instance_id}/auto_progress"))
            .json(&AutoProgressConfig {
                artificial_delay_ms: Some(artificial_delay as u64),
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub(crate) async fn create_http_gateway(
        &self,
        forward_to: HttpGatewayBackend,
        port: Option<u16>,
    ) -> Result<HttpGatewayInfo, CreateHttpGatewayError> {
        let resp = self
            .post("/http_gateway")
            .json(&HttpGatewayConfig {
                ip_addr: None,
                port,
                forward_to,
                domains: None,
                https_config: None,
            })
            .send()
            .await?
            .error_for_status()?
            .json::<CreateHttpGatewayResponse>()
            .await?;

        match resp {
            CreateHttpGatewayResponse::Error { message } => {
                Err(CreateHttpGatewayError::Create { message })
            }
            CreateHttpGatewayResponse::Created(info) => Ok(info),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum CreateInstanceError {
    #[snafu(
        display("failed to create PocketIC instance: {message}"),
        context(suffix(InstanceSnafu))
    )]
    Create { message: String },

    #[snafu(transparent)]
    Reqwest { source: reqwest::Error },
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
