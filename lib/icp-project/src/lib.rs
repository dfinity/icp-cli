use icp_network::structure::NetworkDirectoryStructure;
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};
use std::path::PathBuf;

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
#[derive(Debug, Deserialize)]
pub struct ProjectManifest {
    /// List of canister manifests belonging to this project.
    /// Supports glob patterns to specify multiple canister YAML files.
    #[serde(default)]
    pub canisters: Vec<PathBuf>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    #[serde(default)]
    pub networks: Vec<PathBuf>,
}

impl ProjectManifest {
    pub fn from_bytes<B: AsRef<[u8]>>(bytes: B) -> Result<Self, ProjectManifestError> {
        let mut pm: ProjectManifest = serde_yaml::from_slice(bytes.as_ref()).context(ParseSnafu)?;

        // Project canisters
        let mut cs = Vec::new();

        for pattern in pm.canisters {
            let pattern = pattern.to_str().context(InvalidPathUtf8Snafu)?;
            let matches = glob::glob(pattern).context(GlobPatternSnafu)?;

            for c in matches {
                let path = c.context(GlobWalkSnafu)?;

                // Skip non-canister directories
                if !path.join("canister.yaml").exists() {
                    continue;
                }

                cs.push(path);
            }
        }

        pm.canisters = cs;

        Ok(pm)
    }
}

#[derive(Debug, Snafu)]
pub enum ProjectManifestError {
    #[snafu(display("failed to parse project manifest: {}", source))]
    Parse { source: serde_yaml::Error },

    #[snafu(display("invalid UTF-8 in canister path"))]
    InvalidPathUtf8,

    #[snafu(display("invalid glob pattern in manifest: {}", source))]
    GlobPattern { source: glob::PatternError },

    #[snafu(display("failed while reading glob matches: {}", source))]
    GlobWalk { source: glob::GlobError },
}

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
