use crate::structure::ProjectDirectoryStructure;
use camino::{Utf8Path, Utf8PathBuf};
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RawCanistersField {
    Canister(CanisterManifest),
    Canisters(Vec<Utf8PathBuf>),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanistersField {
    Canister((Utf8PathBuf, CanisterManifest)),
    Canisters(Vec<(Utf8PathBuf, CanisterManifest)>),
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
#[derive(Debug, Deserialize)]
pub struct ProjectManifest {
    /// List of canister manifests belonging to this project.
    pub canisters: CanistersField,

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
    /// The `canisters` field in the manifest supports glob patterns to specify
    /// multiple canister YAML files. Even if a direct path to a canister directory
    /// is provided (e.g., `canisters/my_canister`), it will be processed as a glob.
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

        // Load the raw project manifest from the icp.yaml file.
        let pm: RawProjectManifest = load_yaml_file(mpath)?;

        // Resolve the canisters field: if not explicitly defined in the YAML (i.e., None),
        // fall back to the default glob pattern for locating canister manifests.
        let canisters_field = pm.canisters.unwrap_or_else(default_canisters);

        // Process the resolved RawCanistersField into the final CanistersField.
        let cs = match canisters_field {
            // Case 1: Single-canister project, where 'canister' key was used.
            RawCanistersField::Canister(c) => CanistersField::Canister((
                pds.root().to_owned(), // path
                c,                     // manifest
            )),

            // Case 2: Multi-canister project, where 'canisters' key was used (or default applied).
            RawCanistersField::Canisters(cs) => {
                let mut out = vec![];

                for pattern in cs {
                    // TODO(or.ricon): We should check if the pattern is a glob
                    //   If it's not, we should raise an error when the specified path
                    //   does not represent a canister directory (e.g doesnt contain a canister.yaml file)
                    let matches = glob::glob(pds.root().join(&pattern).as_str())
                        .context(GlobPatternSnafu { pattern })?;

                    for cpath in matches {
                        // Directory path of the found canister.
                        let cpath = cpath.context(GlobWalkSnafu { path: mpath })?;
                        let path: Utf8PathBuf = cpath.try_into()?;

                        // Load the canister manifest from the resolved path.
                        let mpath = pds.canister_yaml_path(&path);
                        let cm = CanisterManifest::load(&mpath)
                            .context(CanisterLoadSnafu { path: &path })?;

                        out.push((
                            path, // path
                            cm,   // manifest
                        ))
                    }
                }

                CanistersField::Canisters(out)
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

    #[snafu(display("failed to glob pattern '{pattern}'"))]
    GlobPattern {
        source: glob::PatternError,
        pattern: String,
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
