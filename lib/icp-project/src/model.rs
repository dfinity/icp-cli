use crate::structure::ProjectDirectoryStructure;
use camino::{Utf8Path, Utf8PathBuf};
use icp_fs::yaml::{LoadYamlFileError, load_yaml_file};
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu};

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

impl TryFrom<&ProjectDirectoryStructure> for ProjectManifest {
    type Error = LoadProjectManifestError;

    fn try_from(pds: &ProjectDirectoryStructure) -> Result<Self, Self::Error> {
        let mpath = pds.project_yaml_path();
        let mpath: &Utf8Path = mpath.as_ref();

        // Load
        let mut pm: ProjectManifest = load_yaml_file(mpath)?;

        // Canisters
        let mut cs = Vec::new();

        for pattern in pm.canisters {
            let mdir = mpath
                .parent()
                .context(ProjectDirectorySnafu { path: mpath })?;

            let matches =
                glob::glob(mdir.join(&pattern).as_str()).context(GlobPatternSnafu { pattern })?;

            for cpath in matches {
                let cpath = cpath.context(GlobWalkSnafu { path: mpath })?;

                let path: Utf8PathBuf = cpath.try_into()?;

                // Skip non-canister directories
                if !pds.canister_yaml_path(&path).exists() {
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
    #[snafu(display("failed to find project directory for project manifest {path}"))]
    ProjectDirectory { path: Utf8PathBuf },

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
