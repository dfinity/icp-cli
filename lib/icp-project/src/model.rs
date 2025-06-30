use icp_canister::model::CanisterManifest;
use serde::Deserialize;

/// Provides the default glob pattern for locating canister manifests
/// when no `canisters` are explicitly specified in the YAML.
pub fn default_canisters() -> CanistersField {
    CanistersField::Canisters(
        ["canisters/*"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
    )
}

/// Provides the default glob pattern for locating network definition files
/// when the `networks` field is not explicitly specified in the YAML.
pub fn default_networks() -> Vec<String> {
    ["networks/*"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanistersField {
    Canister(CanisterManifest),
    Canisters(Vec<String>),
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
    pub networks: Option<Vec<String>>,
}
