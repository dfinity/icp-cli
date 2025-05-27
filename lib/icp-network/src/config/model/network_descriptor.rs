use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorModel {
    pub id: Uuid,
    pub path: PathBuf,
    pub gateway_port: Option<u16>,
    pub pid: Option<u32>,
    pub root_key: String,
}
