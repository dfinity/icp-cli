use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkDescriptorModel {
    pub id: Uuid,
    pub path: Utf8PathBuf,
    pub gateway_port: Option<u16>,
    pub pid: Option<u32>,
    pub root_key: String,
}
