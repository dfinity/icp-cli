use crate::config::model::connected::ConnectedNetworkModel;
use crate::config::model::managed::ManagedNetworkModel;
use serde::Deserialize;

pub type NetworkName = String;

#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum NetworkConfig {
    Managed(ManagedNetworkModel),
    Connected(ConnectedNetworkModel),
}

impl NetworkConfig {
    pub fn local_default() -> Self {
        NetworkConfig::Managed(ManagedNetworkModel::default())
    }
}
