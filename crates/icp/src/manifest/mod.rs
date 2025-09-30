use serde::Deserialize;

use crate::manifest::{
    environment::{CanisterSelection, Environment},
    network::{Configuration, Gateway, Network},
    project::{Canisters, Environments, Networks},
};

mod canister;
mod environment;
mod network;
mod project;
mod recipe;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Item<T> {
    /// Path to a manifest
    Path(String),

    /// The manifest
    Manifest(T),
}

impl Default for Canisters {
    fn default() -> Self {
        Canisters::Canisters(vec![Item::Path("canisters/*".into())])
    }
}

impl Default for Networks {
    fn default() -> Self {
        Networks::Networks(vec![
            Item::Manifest(Network {
                name: "local".to_string(),
                configuration: Configuration::Managed(network::Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: network::Port::Fixed(8080),
                    },
                }),
            }),
            Item::Manifest(Network {
                name: "mainnet".to_string(),
                configuration: Configuration::Connected(network::Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: None,
                }),
            }),
        ])
    }
}

impl Default for Environments {
    fn default() -> Self {
        Environments::Environments(vec![Environment {
            name: "local".to_string(),
            network: "local".to_string(),
            canisters: CanisterSelection::Everything,
            settings: None,
        }])
    }
}
