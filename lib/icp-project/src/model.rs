use crate::structure::ProjectDirectoryStructure;
use camino::{Utf8Path, Utf8PathBuf};
use glob::GlobError;
use icp_canister::model::CanisterManifest;
use icp_fs::yaml::{LoadYamlFileError, load_yaml_file};
use serde::Deserialize;
use snafu::{ResultExt, Snafu};

/// Provides the default glob pattern for locating canister manifests
/// when no `canisters` are explicitly specified in the YAML.
fn default_canisters() -> RawCanistersField {
    RawCanistersField::Canisters(["canisters/*"].iter().map(Utf8PathBuf::from).collect())
}

/// Provides the default glob pattern for locating network definition files
/// when the `networks` field is not explicitly specified in the YAML.
fn default_networks() -> Vec<Utf8PathBuf> {
    ["networks/*"].iter().map(Utf8PathBuf::from).collect()
}

fn is_glob<P: AsRef<Utf8Path>>(path: P) -> bool {
    let s = path.as_ref().as_str();
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RawCanistersField {
    Canister(CanisterManifest),
    Canisters(Vec<Utf8PathBuf>),
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
#[derive(Debug, Deserialize)]
pub struct RawProjectManifest {
    /// Canister manifests belonging to this project.
    /// This field uses `#[serde(flatten)]` to allow deserialization from either
    /// a top-level `canister` key (for a single canister) or a `canisters` key
    /// (for multiple canisters, supporting glob patterns).
    /// If neither key is present, it defaults to `None`, which is then handled
    /// by the `ProjectManifest::load` function to apply a default glob pattern.
    #[serde(flatten)]
    pub canisters: Option<RawCanistersField>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    #[serde(default = "default_networks")]
    pub networks: Vec<Utf8PathBuf>,
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
pub struct ProjectManifest {
    /// List of canister manifests belonging to this project.
    pub canisters: Vec<(Utf8PathBuf, CanisterManifest)>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    pub networks: Vec<Utf8PathBuf>,
}

impl ProjectManifest {
    /// Loads the project manifest (`icp.yaml`) and resolves canister paths.
    ///
    /// This function utilizes the provided [`ProjectDirectoryStructure`] to locate
    /// the `icp.yaml` file. It then deserializes the `canister` or `canisters`
    /// field, handling both single-canister and multi-canister configurations.
    /// If neither `canister` nor `canisters` is explicitly defined in the YAML,
    /// a default glob pattern (`canisters/*`) is applied.
    ///
    /// # Canister Path Resolution
    ///
    /// The `canisters` field supports both glob patterns and explicit paths to define
    /// which canisters are part of the project.
    ///
    /// - **Glob Patterns**: Paths containing wildcards (e.g., `*`, `?`) are treated
    ///   as glob patterns. They are expanded to find all matching directories that
    ///   contain a `canister.yaml` file. Directories that match the glob but do not
    ///   contain a manifest are silently ignored.
    ///
    /// - **Explicit Paths**: Paths without wildcards are treated as explicit references
    ///   to canister directories. For each explicit path, the function verifies that:
    ///     1. The path exists and is a directory.
    ///     2. The directory contains a `canister.yaml` manifest file.
    ///
    /// If an explicit path fails these checks, the loading process will return an
    /// error, providing clear feedback for misconfigured manifests.
    pub fn load(pds: &ProjectDirectoryStructure) -> Result<Self, LoadProjectManifestError> {
        let mpath = pds.project_yaml_path();
        let mpath: &Utf8Path = mpath.as_ref();

        // Load the raw project manifest from the icp.yaml file.
        let pm: RawProjectManifest = load_yaml_file(mpath)?;

        // Resolve the canisters field: if not explicitly defined in the YAML (i.e., None),
        // fall back to the default glob pattern for locating canister manifests.
        let canisters_field = pm.canisters.unwrap_or_else(default_canisters);

        // Process the resolved RawCanistersField into the final CanistersField.
        let cs = match canisters_field {
            // Case 1: Single-canister project, where 'canister' key was used.
            RawCanistersField::Canister(c) => vec![(
                pds.root().to_owned(), // path
                c,                     // manifest
            )],

            // Case 2: Multi-canister project, where 'canisters' key was used (or default applied).
            RawCanistersField::Canisters(cs) => {
                let mut out = vec![];

                for pattern in cs {
                    let dirs = match is_glob(&pattern) {
                        // Glob
                        true => {
                            // Resolve glob
                            let matches = glob::glob(pds.root().join(&pattern).as_str())
                                .context(GlobPatternSnafu { pattern: &pattern })?;

                            // Extract values
                            let paths = matches
                                .collect::<Result<Vec<_>, GlobError>>()
                                .context(GlobWalkSnafu { path: &pattern })?;

                            // Convert to Utf8 paths
                            let paths = paths
                                .into_iter()
                                .map(Utf8PathBuf::try_from)
                                .collect::<Result<Vec<_>, _>>()?;

                            // Skip non-canister directories
                            paths
                                .into_iter()
                                .filter(|path| pds.canister_yaml_path(path).exists())
                                .collect()
                        }

                        // Explicit path
                        false => {
                            // Resolve the explicit path against the project root.
                            let canister_path = pds.root().join(&pattern);

                            // Check if path exists and that it's a directory.
                            if !canister_path.is_dir() {
                                return Err(LoadProjectManifestError::CanisterPath {
                                    path: pattern,
                                });
                            }

                            // Check for a canister manifest.
                            if !pds.canister_yaml_path(&canister_path).exists() {
                                return Err(LoadProjectManifestError::NoManifest { path: pattern });
                            }

                            vec![canister_path]
                        }
                    };

                    // Iterate over canister directories
                    for cpath in dirs {
                        // Canister manifest path
                        let mpath = pds.canister_yaml_path(&cpath);

                        // Load the canister manifest from the resolved path.
                        let cm = CanisterManifest::load(&mpath)
                            .context(CanisterLoadSnafu { path: &cpath })?;

                        out.push((
                            cpath, // path
                            cm,    // manifest
                        ))
                    }
                }

                out
            }
        };

        Ok(ProjectManifest {
            // The resolved canister configurations.
            canisters: cs,

            // Network definitions for the project.
            networks: pm.networks,
        })
    }
}

#[derive(Debug, Snafu)]
pub enum LoadProjectManifestError {
    #[snafu(transparent)]
    Parse { source: LoadYamlFileError },

    #[snafu(transparent)]
    InvalidPathUtf8 { source: camino::FromPathBufError },

    #[snafu(display("canister path must exist and be a directory '{path}'"))]
    CanisterPath { path: Utf8PathBuf },

    #[snafu(display("canister path does not contain a manifest '{path}'"))]
    NoManifest { path: Utf8PathBuf },

    #[snafu(display("failed to glob pattern '{pattern}'"))]
    GlobPattern {
        source: glob::PatternError,
        pattern: Utf8PathBuf,
    },

    #[snafu(display("failed to glob pattern in '{path}'"))]
    GlobWalk {
        source: glob::GlobError,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to load canister manifest in path '{path}'"))]
    CanisterLoad {
        source: icp_canister::model::LoadCanisterManifestError,
        path: Utf8PathBuf,
    },
}
