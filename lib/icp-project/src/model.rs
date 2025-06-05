use crate::structure::ProjectDirectoryStructure;
use camino::{Utf8Path, Utf8PathBuf};
use icp_fs::yaml::{LoadYamlFileError, load_yaml_file};
use serde::Deserialize;
use snafu::{ResultExt, Snafu};

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
    /// Loads the project manifest (`project.yaml`) and resolves canister paths.
    ///
    /// This function utilizes the provided [`ProjectDirectoryStructure`] to locate
    /// the `project.yaml` file and then identify all canister directories
    /// referenced within it.
    ///
    /// # Canister Path Resolution
    ///
    /// Currently, all paths specified in the `canisters` field of the manifest
    /// are treated as glob patterns. This means that even if a direct path to a
    /// canister directory is provided (e.g., `canisters/my_canister`), it will
    /// be processed as a glob.
    ///
    /// A consequence of this glob-based approach is that if an explicitly
    /// specified canister path does not contain a `canister.yaml` file (thus,
    /// not being a valid canister directory according to `ProjectDirectoryStructure`),
    /// it will be silently ignored rather than causing an error.
    ///
    /// **Future Improvement:** This behavior should be changed. In a future
    /// version, if a path in the `canisters` list is *not* a glob pattern (i.e.,
    /// it's an explicit path), and that path does not point to a valid canister
    /// directory (i.e., it's missing a `canister.yaml` or is not a directory),
    /// the loading process should raise an error. This will provide clearer
    /// feedback for misconfigured manifests.
    pub fn load(pds: &ProjectDirectoryStructure) -> Result<Self, LoadProjectManifestError> {
        let mpath = pds.project_yaml_path();
        let mpath: &Utf8Path = mpath.as_ref();

        // Load
        let mut pm: ProjectManifest = load_yaml_file(mpath)?;

        // Canisters
        let mut cs = Vec::new();

        for pattern in pm.canisters {
            let matches = glob::glob(pds.root().join(&pattern).as_str())
                .context(GlobPatternSnafu { pattern })?;

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
