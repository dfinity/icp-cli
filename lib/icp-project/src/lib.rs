use icp_network::structure::NetworkDirectoryStructure;
use serde::Deserialize;
use std::path::PathBuf;

pub struct ProjectDirectoryStructure {
    root: PathBuf,
}

impl ProjectDirectoryStructure {
    pub fn find() -> Option<Self> {
        let current_dir = std::env::current_dir().ok()?;
        let mut path = current_dir.clone();
        loop {
            if path.join("icp.yaml").exists() {
                break Some(Self { root: path });
            }
            if !path.pop() {
                break None;
            }
        }
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    #[allow(dead_code)]
    pub fn network_config_path(&self, name: &str) -> PathBuf {
        self.root.join("networks").join(format!("{name}.yaml"))
    }

    fn work_dir(&self) -> PathBuf {
        self.root.join(".icp")
    }

    pub fn network(&self, network_name: &str) -> NetworkDirectoryStructure {
        let network_root = self.work_dir().join("networks").join(network_name);

        NetworkDirectoryStructure::new(&network_root)
    }
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
#[derive(Debug, Deserialize)]
pub struct ProjectManifest {
    /// List of canister manifests belonging to this project.
    /// Supports glob patterns to specify multiple canister YAML files.
    pub canisters: Vec<PathBuf>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    pub networks: Vec<PathBuf>,
}
