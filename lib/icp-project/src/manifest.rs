use std::collections::HashMap;

use serde::Deserialize;

use icp_canister::{CanisterManifest, CanisterSettings};
use icp_network::NetworkConfig;

/// Provides the default glob pattern for locating canister manifests
/// when no `canisters` are explicitly specified in the YAML.
pub fn default_canisters() -> CanistersField {
    CanistersField::Canisters(
        ["canisters/*"]
            .into_iter()
            .map(String::from)
            .map(CanisterItem::Path)
            .collect::<Vec<_>>(),
    )
}

/// Provides the default glob pattern for locating network definition files
/// when the `networks` field is not explicitly specified in the YAML.
pub fn default_networks() -> Vec<NetworkItem> {
    ["networks/*"]
        .into_iter()
        .map(String::from)
        .map(NetworkItem::Path)
        .collect::<Vec<_>>()
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum CanisterItem {
    Path(String),
    Definition(CanisterManifest),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum CanistersField {
    Canister(CanisterManifest),
    Canisters(Vec<CanisterItem>),
}

#[derive(Clone, Debug, Deserialize)]
pub struct NetworkManifest {
    pub name: String,

    #[serde(flatten)]
    pub config: NetworkConfig,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum NetworkItem {
    Path(String),
    Definition(NetworkManifest),
}

#[derive(Clone, Debug, Deserialize)]
pub struct EnvironmentManifest {
    // environment name
    pub name: String,

    // target network for canister deployment
    pub network: Option<String>,

    // canisters the environment should contain
    pub canisters: Option<Vec<String>>,

    // canister settings overrides
    pub settings: Option<HashMap<String, CanisterSettings>>,
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
#[derive(Debug, Deserialize)]
pub struct ProjectManifest {
    /// Canister manifests belonging to this project.
    /// This field uses `#[serde(flatten)]` to allow deserialization from either
    /// a top-level `canister` key (for a single canister) or a `canisters` key
    /// (for multiple canisters, supporting glob patterns).
    /// If neither key is present, it defaults to `None`, which is then handled
    /// by the `ProjectManifest::load` function to apply a default glob pattern.
    #[serde(flatten)]
    pub canisters: Option<CanistersField>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    pub networks: Option<Vec<NetworkItem>>,

    // Projects define environments to which canisters can be deployed
    // An environment is always associated with a project-defined network
    pub environments: Option<Vec<EnvironmentManifest>>,
}
