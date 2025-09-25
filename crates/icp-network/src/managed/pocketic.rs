use candid::Principal;
use icp::prelude::*;
use pocket_ic::common::rest::{
    AutoProgressConfig, CreateHttpGatewayResponse, CreateInstanceResponse, HttpGatewayBackend,
    HttpGatewayConfig, HttpGatewayInfo, IcpConfig, IcpConfigFlag, IcpFeatures, IcpFeaturesConfig,
    InstanceConfig, InstanceId, RawTime, SubnetConfigSet, Topology,
};
use reqwest::Url;
use snafu::prelude::*;
use time::OffsetDateTime;

#[allow(dead_code)]
pub struct PocketIcInstance {
    pub admin: PocketIcAdminInterface,
    pub gateway_port: u16,
    pub instance_id: InstanceId,
    pub effective_canister_id: Principal,
    pub root_key: String,
}

pub fn spawn_pocketic(pocketic_path: &Path, port_file: &Path) -> tokio::process::Child {
    let mut cmd = tokio::process::Command::new(pocketic_path);
    cmd.arg("--port-file");
    cmd.arg(port_file.as_os_str());
    cmd.args(["--ttl", "2592000", "--log-levels", "error"]);

    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    #[cfg(unix)]
    {
        //use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    eprintln!("Starting PocketIC...");
    cmd.spawn().expect("Could not start PocketIC.")
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

    pub async fn create_instance(
        &self,
        state_dir: &Path,
    ) -> Result<(InstanceId, Topology), CreateInstanceError> {
        // Specify configuration for network
        let inst_cfg = InstanceConfig {
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

                // The rest of the features are disabled by default
                ..Default::default()
            }),

            subnet_config_set: (SubnetConfigSet {
                // Configure a single application subnet
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
        };

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
