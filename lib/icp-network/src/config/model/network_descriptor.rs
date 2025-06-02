use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorGatewayPort {
    pub fixed: bool,
    pub port: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorModel {
    pub id: Uuid,
    pub project_dir: Utf8PathBuf,
    pub network: String,
    pub network_dir: Utf8PathBuf,
    pub gateway: NetworkDescriptorGatewayPort,
    pub pid: Option<u32>,
    pub root_key: String,
}

impl NetworkDescriptorModel {
    pub fn gateway_port(&self) -> Option<u16> {
        if self.gateway.fixed {
            Some(self.gateway.port)
        } else {
            None
        }
    }
}
