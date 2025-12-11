use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::prelude::*;

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
    pub project_dir: PathBuf,
    pub network: String,
    pub network_dir: PathBuf,
    pub gateway: NetworkDescriptorGatewayPort,
    pub pid: Option<u32>,
    #[serde(with = "hex::serde")]
    pub root_key: Vec<u8>,
}

impl NetworkDescriptorModel {
    pub fn gateway_port(&self) -> Option<u16> {
        if self.gateway.fixed {
            return Some(self.gateway.port);
        }

        None
    }
}
