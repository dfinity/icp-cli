use std::path::{Path, PathBuf};

use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};

use icp_fs::fs::{ReadFileError, read};

/// Provides the default glob pattern for locating canister manifests
/// when the `canisters` field is not explicitly specified in the YAML.
fn default_canisters() -> Vec<PathBuf> {
    ["canisters/*"].iter().map(PathBuf::from).collect()
}

/// Provides the default glob pattern for locating network definition files
/// when the `networks` field is not explicitly specified in the YAML.
fn default_networks() -> Vec<PathBuf> {
    ["networks/*"].iter().map(PathBuf::from).collect()
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
#[derive(Debug, Deserialize)]
pub struct ProjectManifest {
    /// List of canister manifests belonging to this project.
    /// Supports glob patterns to specify multiple canister YAML files.
    #[serde(default = "default_canisters")]
    pub canisters: Vec<PathBuf>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    #[serde(default = "default_networks")]
    pub networks: Vec<PathBuf>,
}

impl ProjectManifest {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, LoadProjectManifestError> {
        let path = path.as_ref();

        // Load
        let bytes = read(path)?;

        // Parse
        let mut pm: ProjectManifest =
            serde_yaml::from_slice(bytes.as_ref()).context(ParseSnafu { path })?;

        // Project canisters
        let mut cs = Vec::new();

        for pattern in pm.canisters {
            let patcpy = pattern.clone();

            let pattern = pattern
                .to_str()
                .context(InvalidPathUtf8Snafu { pattern: patcpy })?;

            let matches = glob::glob(pattern).context(GlobPatternSnafu { pattern })?;

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
pub enum LoadProjectManifestError {
    #[snafu(display("failed to parse {}", path.display()))]
    Parse {
        source: serde_yaml::Error,
        path: PathBuf,
    },

    #[snafu(display("invalid UTF-8 in canister path pattern {}", pattern.display()))]
    InvalidPathUtf8 { pattern: PathBuf },

    #[snafu(display("failed to glob pattern {pattern}"))]
    GlobPattern {
        source: glob::PatternError,
        pattern: String,
    },

    /// GlobWalk is transparent because `glob::GlobError` already contains the path.
    #[snafu(transparent)]
    GlobWalk { source: glob::GlobError },

    #[snafu(transparent)]
    ReadFile { source: ReadFileError },
}
