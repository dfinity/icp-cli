use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use snafu::{ResultExt, Snafu};

use icp_fs::yaml::{LoadYamlFileError, load_yaml_file};

/// Provides the default glob pattern for locating canister manifests
/// when the `canisters` field is not explicitly specified in the YAML.
fn default_canisters() -> Vec<Utf8PathBuf> {
    ["canisters/*"].iter().map(Utf8PathBuf::from).collect()
}

/// Provides the default glob pattern for locating network definition files
/// when the `networks` field is not explicitly specified in the YAML.
fn default_networks() -> Vec<Utf8PathBuf> {
    ["networks/*"].iter().map(Utf8PathBuf::from).collect()
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
#[derive(Debug, Deserialize)]
pub struct ProjectManifest {
    /// List of canister manifests belonging to this project.
    /// Supports glob patterns to specify multiple canister YAML files.
    #[serde(default = "default_canisters")]
    pub canisters: Vec<Utf8PathBuf>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    #[serde(default = "default_networks")]
    pub networks: Vec<Utf8PathBuf>,
}

impl ProjectManifest {
    pub fn from_file<P: AsRef<Utf8Path>>(path: P) -> Result<Self, LoadProjectManifestError> {
        let mpath = path.as_ref();

        // Load
        let mut pm: ProjectManifest = load_yaml_file(mpath)?;

        // Project canisters
        let mut cs = Vec::new();

        for pattern in pm.canisters {
            let matches = glob::glob(pattern.as_str()).context(GlobPatternSnafu { pattern })?;

            for cpath in matches {
                let cpath = cpath.context(GlobWalkSnafu { path: mpath })?;

                let path: Utf8PathBuf = cpath.try_into()?;

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
    #[snafu(transparent)]
    Parse { source: LoadYamlFileError },

    #[snafu(transparent)]
    InvalidPathUtf8 { source: camino::FromPathBufError },

    #[snafu(display("failed to glob pattern {pattern}"))]
    GlobPattern {
        source: glob::PatternError,
        pattern: String,
    },

    #[snafu(display("failed to glob pattern in {path}"))]
    GlobWalk {
        source: glob::GlobError,
        path: Utf8PathBuf,
    },
}
