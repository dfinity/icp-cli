use candid::Principal;
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
    pub v: String,
    pub id: Uuid,
    pub project_dir: PathBuf,
    pub network: String,
    pub network_dir: PathBuf,
    pub gateway: NetworkDescriptorGatewayPort,
    pub child_locator: ChildLocator,
    #[serde(with = "hex::serde")]
    pub root_key: Vec<u8>,
    pub pocketic_config_port: Option<u16>,
    pub pocketic_instance_id: Option<usize>,
    pub candid_ui_canister_id: Option<Principal>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum ChildLocator {
    Pid {
        pid: u32,
    },
    Container {
        id: String,
        socket: PathBuf,
        rm_on_exit: bool,
    },
}

impl NetworkDescriptorModel {
    pub fn gateway_port(&self) -> Option<u16> {
        if self.gateway.fixed {
            return Some(self.gateway.port);
        }

        None
    }
}
