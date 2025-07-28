use camino::Utf8Path;
use candid::Principal;
use pocket_ic::common::rest::{
    AutoProgressConfig, CreateHttpGatewayResponse, CreateInstanceResponse, ExtendedSubnetConfigSet,
    HttpGatewayBackend, HttpGatewayConfig, HttpGatewayInfo, InstanceConfig, InstanceId, RawTime,
    SubnetSpec, Topology,
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

pub fn spawn_pocketic(pocketic_path: &Utf8Path, port_file: &Utf8Path) -> tokio::process::Child {
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
    base_url: Url,
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
        state_dir: &Utf8Path,
    ) -> Result<(InstanceId, Topology), CreateInstanceError> {
        let subnet_config_set = ExtendedSubnetConfigSet {
            nns: Some(SubnetSpec::default()),
            sns: Some(SubnetSpec::default()),
            ii: Some(SubnetSpec::default()),
            fiduciary: Some(SubnetSpec::default()),
            bitcoin: Some(SubnetSpec::default()),
            system: vec![],
            verified_application: vec![],
            application: vec![<_>::default()],
        };
        let resp = self
            .post("/instances")
            .json(&InstanceConfig {
                subnet_config_set,
                state_dir: Some(state_dir.to_path_buf().into()),
                nonmainnet_features: true,
                log_level: Some("ERROR".to_string()),
                bitcoind_addr: None, // bitcoind_addr.clone(),
            })
            .send()
            .await?
            .error_for_status()?
            .json::<CreateInstanceResponse>()
            .await?;
        match resp {
            CreateInstanceResponse::Error { message } => {
                Err(CreateInstanceError::Create { message })
            }
            CreateInstanceResponse::Created {
                instance_id,
                topology,
            } => Ok((instance_id, topology)),
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
