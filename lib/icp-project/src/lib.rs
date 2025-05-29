use std::path::{Path, PathBuf};

use serde::Deserialize;
use snafu::{OptionExt, Snafu, ensure};

use icp_fs::fs::{ReadFileError, read};

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
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ProjectManifestError> {
        let path = path.as_ref();

        // Check existence
        ensure!(path.exists(), NotFoundSnafu { path });

        // Load
        let bytes = read(path)?;

        // Parse
        let mut pm: ProjectManifest = serde_yaml::from_slice(bytes.as_ref())?;

        // Project canisters
        let mut cs = Vec::new();

        for pattern in pm.canisters {
            let pattern = pattern.to_str().context(InvalidPathUtf8Snafu)?;
            let matches = glob::glob(pattern)?;

            for path in matches {
                let path = path?;

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
    #[snafu(display("project manifest not found: {path:?}"))]
    NotFound { path: PathBuf },

    #[snafu(transparent)]
    Parse { source: serde_yaml::Error },

    #[snafu(display("invalid UTF-8 in canister path"))]
    InvalidPathUtf8,

    #[snafu(transparent)]
    GlobPattern { source: glob::PatternError },

    #[snafu(transparent)]
    GlobWalk { source: glob::GlobError },

    #[snafu(transparent)]
    ReadFile { source: ReadFileError },
}
