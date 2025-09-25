use std::collections::HashMap;

use schemars::JsonSchema;
use serde::Deserialize;

use icp_canister::{CanisterManifest, CanisterSettings};
use icp_network::NetworkConfig;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Item<T> {
    Path(String),
    Manifest(T),
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct NetworkManifest {
    pub name: String,

    #[serde(flatten)]
    pub config: NetworkConfig,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum CanistersField {
    Canister(CanisterManifest),
    Canisters(Vec<Item<CanisterManifest>>),
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
#[derive(Debug, Deserialize, JsonSchema)]
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
    pub networks: Option<Vec<Item<NetworkManifest>>>,

    // Projects define environments to which canisters can be deployed
    // An environment is always associated with a project-defined network
    pub environments: Option<Vec<EnvironmentManifest>>,
}
