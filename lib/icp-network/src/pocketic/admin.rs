use std::path::Path;
use pocket_ic::common::rest::{CreateInstanceResponse, ExtendedSubnetConfigSet, InstanceConfig, InstanceId, SubnetSpec, Topology};
use reqwest::Url;
use snafu::prelude::*;

pub struct PocketIcAdminInterface {
    client: reqwest::Client,
    base_url: Url,
}

#[derive(Debug, Snafu)]
pub enum CreateInstanceError {
    #[snafu(display("failed to create PocketIC instance: {message}"))]
    Create { message: String },

    #[snafu(display("failed to send request"))]
    Reqwest { source: reqwest::Error },
}

impl PocketIcAdminInterface {
    pub fn new(base_url: Url) -> Self {
        let client = reqwest::Client::new();
        Self { client, base_url }
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.post(self.base_url.join(path).unwrap())
    }

    pub async fn create_instance(&self, state_dir: &Path) -> Result<(InstanceId, Topology), CreateInstanceError> {
        let mut subnet_config_set = ExtendedSubnetConfigSet {
            nns: Some(SubnetSpec::default()),
            sns: Some(SubnetSpec::default()),
            ii: Some(SubnetSpec::default()),
            fiduciary: Some(SubnetSpec::default()),
            bitcoin: Some(SubnetSpec::default()),
            system: vec![],
            verified_application: vec![],
            application: vec![<_>::default()],
        };
        // match replica_config.subnet_type {
        //     ReplicaSubnetType::Application => subnet_config_set.application.push(<_>::default()),
        //     ReplicaSubnetType::System => subnet_config_set.system.push(<_>::default()),
        //     ReplicaSubnetType::VerifiedApplication => {
        //         subnet_config_set.verified_application.push(<_>::default())
        //     }
        // }
        eprintln!("Creating instance");
        let resp = self
            .post("/instances")
            .json(&InstanceConfig {
                subnet_config_set,
                state_dir: Some(state_dir.to_path_buf()),
                nonmainnet_features: true,
                log_level: Some("ERROR".to_string()),
                bitcoind_addr: None, // bitcoind_addr.clone(),
            })
            .send()
            .await.context(ReqwestSnafu)?
            .error_for_status().context(ReqwestSnafu)?
            .json::<CreateInstanceResponse>()
            .await.context(ReqwestSnafu)?;
        match resp {
            CreateInstanceResponse::Error { message } => {
                return Err(CreateInstanceError::Create { message });
            }
            CreateInstanceResponse::Created {
                instance_id,
                topology,
            } => {
                // let default_effective_canister_id: Principal =
                //     topology.default_effective_canister_id.into();

                Ok((instance_id, topology))
            }
        }
    }
}
