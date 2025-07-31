use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    str::FromStr,
};

use camino::{Utf8Path, Utf8PathBuf};
use glob::GlobError;
use icp_canister::manifest::CanisterManifest;
use icp_fs::yaml::LoadYamlFileError;
use icp_network::{NETWORK_IC, NETWORK_LOCAL, NetworkConfig};
use pathdiff::diff_utf8_paths;
use snafu::{ResultExt, Snafu};

use crate::{
    ENVIRONMENT_LOCAL,
    directory::ProjectDirectory,
    model::{
        CanisterItem, CanistersField, EnvironmentManifest, NetworkItem, NetworkManifest,
        default_networks,
    },
};

fn is_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

/// Represents the manifest for an ICP project, typically loaded from `icp.yaml`.
/// A project is a repository or directory grouping related canisters and network definitions.
pub struct Project {
    /// Access to the project directory.
    pub directory: ProjectDirectory,

    /// List of canister manifests belonging to this project.
    pub canisters: Vec<(Utf8PathBuf, CanisterManifest)>,

    /// List of network definition files relevant to the project.
    /// Supports glob patterns to reference multiple network config files.
    pub networks: HashMap<String, NetworkConfig>,

    // List of environment definitions as defined by the project.
    pub environments: Vec<EnvironmentManifest>,
}

impl Project {
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
    pub fn load(pd: ProjectDirectory) -> Result<Self, LoadProjectManifestError> {
        let pds = pd.structure();
        let pm = pd.load_project_manifest()?;

        // Resolve the canisters field: if not explicitly defined in the YAML (i.e., None),
        // fall back to the default glob pattern for locating canister manifests.
        let canisters_field = pm.canisters.unwrap_or_else(crate::model::default_canisters);

        // Process the resolved RawCanistersField into the final CanistersField.
        let canisters = match canisters_field {
            // Case 1: Single-canister project, where 'canister' key was used.
            CanistersField::Canister(c) => vec![(
                pds.root().to_owned(), // path
                c,                     // manifest
            )],

            // Case 2: Multi-canister project, where 'canisters' key was used (or default applied).
            CanistersField::Canisters(cs) => {
                // Collect paths and inline-canister definitions
                let (paths, mut cs) = (
                    //
                    // Paths
                    cs.iter()
                        .filter_map(|v| match v {
                            CanisterItem::Path(path) => Some(path.to_owned()),
                            CanisterItem::Definition(_) => None,
                        })
                        .collect::<Vec<String>>(),
                    //
                    // Manifests
                    cs.iter()
                        .filter_map(|v| match v {
                            CanisterItem::Path(_) => None,
                            CanisterItem::Definition(c) => {
                                Some((
                                    pds.root().to_owned(), // path
                                    c.to_owned(),          // canister
                                ))
                            }
                        })
                        .collect::<Vec<(Utf8PathBuf, CanisterManifest)>>(),
                );

                // Track names
                let mut cnames: HashMap<String, ()> = HashMap::new();

                for c in &cs {
                    match cnames.entry(c.1.name.to_owned()) {
                        // Duplicate
                        Entry::Occupied(e) => {
                            return Err(LoadProjectManifestError::DuplicateCanister {
                                canister: e.key().to_owned(),
                            });
                        }

                        // Ok
                        Entry::Vacant(e) => {
                            e.insert(());
                        }
                    }
                }

                // Process paths and globs
                for pattern in paths {
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
                            // Path
                            let path = Utf8PathBuf::from_str(&pattern)
                                .expect("this is an infallible operation");

                            // Resolve the explicit path against the project root.
                            let canister_path = pds.root().join(&path);

                            // Check if path exists and that it's a directory.
                            if !canister_path.is_dir() {
                                return Err(LoadProjectManifestError::CanisterPath { path });
                            }

                            // Check for a canister manifest.
                            let manifest_path = pds.canister_yaml_path(&canister_path);

                            if !manifest_path.exists() {
                                return Err(LoadProjectManifestError::NoManifest {
                                    path: manifest_path,
                                });
                            }

                            vec![canister_path]
                        }
                    };

                    // Iterate over canister directories
                    for cpath in dirs {
                        // Load the canister manifest from the resolved path.
                        let cm = pd
                            .load_canister_manifest(&cpath)
                            .context(CanisterLoadSnafu { path: &cpath })?;

                        // Check for duplicates
                        match cnames.entry(cm.name.to_owned()) {
                            // Duplicate
                            Entry::Occupied(e) => {
                                return Err(LoadProjectManifestError::DuplicateCanister {
                                    canister: e.key().to_owned(),
                                });
                            }

                            // Ok
                            Entry::Vacant(e) => {
                                e.insert(());
                            }
                        }

                        cs.push((
                            cpath, // path
                            cm,    // manifest
                        ))
                    }
                }

                cs
            }
        };

        // Networks
        let networks = pm.networks.unwrap_or(default_networks());

        let (paths, nms) = (
            //
            // Paths
            networks
                .iter()
                .filter_map(|v| match v {
                    NetworkItem::Path(path) => Some(path.to_owned()),
                    NetworkItem::Definition(_) => None,
                })
                .collect::<Vec<String>>(),
            //
            // Manifests
            networks
                .iter()
                .filter_map(|v| match v {
                    NetworkItem::Path(_) => None,
                    NetworkItem::Definition(m) => Some(m.to_owned()),
                })
                .collect::<Vec<NetworkManifest>>(),
        );

        // Collect network definitions
        let mut networks: HashMap<String, NetworkConfig> = HashMap::new();

        // Check for duplicates among inline-defined networks
        for v in &nms {
            match networks.entry(v.name.to_owned()) {
                // Duplicate
                Entry::Occupied(e) => {
                    return Err(LoadProjectManifestError::DuplicateNetwork {
                        network: e.key().to_owned(),
                    });
                }

                // Ok
                Entry::Vacant(e) => {
                    e.insert(v.config.to_owned());
                }
            }
        }

        // Load network paths
        let paths = Project::gather_network_paths(&pd, paths)?;

        // Check for duplicates among path-based networks
        for (name, cfg) in Project::load_network_configurations(&pd, paths)? {
            match networks.entry(name) {
                // Duplicate
                Entry::Occupied(e) => {
                    return Err(LoadProjectManifestError::DuplicateNetwork {
                        network: e.key().to_owned(),
                    });
                }

                // Ok
                Entry::Vacant(e) => {
                    e.insert(cfg);
                }
            }
        }

        // Ensure a `local` network is defined
        networks
            .entry(NETWORK_LOCAL.to_string())
            .or_insert(NetworkConfig::local_default());

        // Environments
        let environments = pm.environments.unwrap_or(vec![EnvironmentManifest {
            name: ENVIRONMENT_LOCAL.to_string(),
            network: None,
            canisters: None,
            settings: None,
        }]);

        // Check for duplicate environments
        let mut enames: HashMap<String, ()> = HashMap::new();

        for v in &environments {
            match enames.entry(v.name.to_owned()) {
                // Duplicate
                Entry::Occupied(e) => {
                    return Err(LoadProjectManifestError::DuplicateEnvironment {
                        environment: e.key().to_owned(),
                    });
                }

                // Ok
                Entry::Vacant(e) => {
                    e.insert(());
                }
            }
        }

        // Default environments network
        let environments = environments
            .into_iter()
            .map(|mut v| match v.network {
                // Explicitly-specified network
                Some(_) => v,

                // Default network for an environment is `local`
                None => {
                    v.network = Some(NETWORK_LOCAL.into());
                    v
                }
            })
            .collect::<Vec<EnvironmentManifest>>();

        // Complain about environments that point to non-existent networks
        for e in &environments {
            if let Some(network) = &e.network {
                if !networks.contains_key(network) {
                    return Err(LoadProjectManifestError::EnvironmentNetworkDoesntExist {
                        environment: e.name.to_owned(),
                        network: network.to_owned(),
                    });
                }
            }
        }

        // Complain about environments that point to non-existent canisters
        let cnames: HashSet<String> = canisters.iter().map(|(_, c)| c.name.to_owned()).collect();

        for e in &environments {
            if let Some(cs) = &e.canisters {
                for cname in cs {
                    // Check against project's canisters
                    if !cnames.contains(cname) {
                        return Err(LoadProjectManifestError::EnvironmentCanisterDoesntExist {
                            environment: e.name.to_owned(),
                            canister: cname.to_owned(),
                        });
                    }
                }
            }
        }

        Ok(Project {
            // The project directory.
            directory: pd,

            // The resolved canister configurations.
            canisters,

            // Network definitions for the project.
            networks,

            // Environment definitions for the project.
            environments,
        })
    }

    // For network paths that are glob patterns, it's ok if they don't match any files.
    // For specific network paths, we make sure the configuration file exists.
    fn gather_network_paths(
        pd: &ProjectDirectory,
        network_paths: Vec<String>,
    ) -> Result<Vec<Utf8PathBuf>, GatherNetworkPathsError> {
        // relative to the project root, not including .yaml extension
        let mut result_paths = vec![];
        for network_path in network_paths {
            if is_glob(&network_path) {
                let mut glob_paths = Self::normalize_glob_networks(&network_path, pd)?;
                result_paths.append(&mut glob_paths);
            } else {
                let path = Utf8PathBuf::from(network_path);
                Self::check_specific_network(&path, pd)?;
                result_paths.push(path);
            }
        }

        Ok(result_paths)
    }

    fn load_network_configurations(
        pd: &ProjectDirectory,
        network_paths: Vec<Utf8PathBuf>,
    ) -> Result<HashMap<String, NetworkConfig>, LoadNetworkConfigurationsError> {
        let mut networks = HashMap::new();

        for network_path in network_paths {
            let name = network_path
                .file_name()
                .ok_or(LoadNetworkConfigurationsError::NoNetworkName {
                    network_path: network_path.clone(),
                })?
                .to_string();

            if name == NETWORK_IC {
                return Err(LoadNetworkConfigurationsError::CannotRedefineIcNetwork {
                    network_path: network_path.clone(),
                });
            }

            if networks.contains_key(&name) {
                return Err(LoadNetworkConfigurationsError::DuplicateNetworkName { name });
            }

            // Load the network config from the path
            let network_config = pd.load_network_config(&network_path)?;

            // Insert into the networks map
            networks.insert(name, network_config);
        }

        Ok(networks)
    }

    // For a pattern like `networks/*`, this function will return all matching network paths,
    // relative to the project root, without the `.yaml` extension.
    // For example, for pattern `networks/*`, it will return
    // paths like `networks/local` or `networks/test`.
    fn normalize_glob_networks(
        pattern: &str,
        pd: &ProjectDirectory,
    ) -> Result<Vec<Utf8PathBuf>, NormalizeGlobNetworksError> {
        let root = pd.structure().root();
        let matches =
            glob::glob(root.join(pattern).as_str()).context(NetworkGlobPatternSnafu { pattern })?;
        let paths = matches
            .collect::<Result<Vec<_>, GlobError>>()
            .context(NetworkGlobWalkSnafu { path: &pattern })?;

        paths
            .into_iter()
            .filter_map(|path| match Utf8PathBuf::try_from(path) {
                Ok(p) if p.extension() == Some("yaml") => {
                    let without_ext = p.with_extension("");
                    let rel = diff_utf8_paths(&without_ext, root)?;
                    Some(Ok(rel))
                }
                Ok(_) => None,
                Err(e) => Some(Err(e.into())),
            })
            .collect()
    }

    // For a specific network path, this function makes sure the
    // network configuration file exists
    fn check_specific_network(
        network_path: &Utf8Path,
        pd: &ProjectDirectory,
    ) -> Result<(), CheckSpecificNetworkError> {
        let config_path = pd.structure().network_config_path(network_path);

        if !config_path.is_file() {
            return Err(CheckSpecificNetworkError::NetworkPath {
                network_path: network_path.to_path_buf(),
                config_path,
            });
        }

        Ok(())
    }

    pub fn get_network_config(
        &self,
        network_name: &str,
    ) -> Result<&NetworkConfig, NoSuchNetworkError> {
        self.networks.get(network_name).ok_or(NoSuchNetworkError {
            network: network_name.to_string(),
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

    #[snafu(display("no canister manifest found at '{path}'"))]
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
        source: LoadYamlFileError,
        path: Utf8PathBuf,
    },

    #[snafu(transparent)]
    GatherNetworkPaths { source: GatherNetworkPathsError },

    #[snafu(transparent)]
    LoadNetworkConfigurations {
        source: LoadNetworkConfigurationsError,
    },

    #[snafu(display("project contains two similarly named canisters: '{canister}'"))]
    DuplicateCanister { canister: String },

    #[snafu(display("project contains two similarly named networks: '{network}'"))]
    DuplicateNetwork { network: String },

    #[snafu(display("project contains two similarly named environments: '{environment}'"))]
    DuplicateEnvironment { environment: String },

    #[snafu(display("environment '{environment}' targets non-existent network '{network}'"))]
    EnvironmentNetworkDoesntExist {
        environment: String,
        network: String,
    },
    #[snafu(display("environment '{environment}' deploys non-existent canistrer '{canister}'"))]
    EnvironmentCanisterDoesntExist {
        environment: String,
        canister: String,
    },
}

#[derive(Debug, Snafu)]
pub enum GatherNetworkPathsError {
    #[snafu(transparent)]
    NormalizeGlobNetworks { source: NormalizeGlobNetworksError },

    #[snafu(transparent)]
    CheckSpecificNetwork { source: CheckSpecificNetworkError },
}

#[derive(Debug, Snafu)]
pub enum LoadNetworkConfigurationsError {
    #[snafu(display(
        "cannot redefine the 'ic' network; the network path '{network_path}' is invalid"
    ))]
    CannotRedefineIcNetwork { network_path: Utf8PathBuf },

    #[snafu(display("duplicate network name found: '{name}'"))]
    DuplicateNetworkName { name: String },

    #[snafu(transparent)]
    LoadYamlFile { source: LoadYamlFileError },

    #[snafu(display("unable to determine network name from path '{network_path}'"))]
    NoNetworkName { network_path: Utf8PathBuf },
}

#[derive(Debug, Snafu)]
pub enum NormalizeGlobNetworksError {
    #[snafu(transparent)]
    InvalidPathUtf8 { source: camino::FromPathBufError },

    #[snafu(display("failed to glob pattern '{pattern}'"))]
    NetworkGlobPattern {
        source: glob::PatternError,
        pattern: Utf8PathBuf,
    },

    #[snafu(display("failed to glob pattern in '{path}'"))]
    NetworkGlobWalk {
        source: glob::GlobError,
        path: Utf8PathBuf,
    },
}

#[derive(Debug, Snafu)]
pub enum CheckSpecificNetworkError {
    #[snafu(transparent)]
    InvalidPathUtf8 { source: camino::FromPathBufError },

    #[snafu(display(
        "configuration file for network '{network_path}' not found at '{config_path}'"
    ))]
    NetworkPath {
        network_path: Utf8PathBuf,
        config_path: Utf8PathBuf,
    },
}

#[derive(Debug, Snafu)]
#[snafu(display("no such network: '{}'", network))]
pub struct NoSuchNetworkError {
    network: String,
}

#[cfg(test)]
mod tests {
    use camino_tempfile::tempdir;

    use icp_adapter::script::{CommandField, ScriptAdapter};
    use icp_canister::{
        BuildStep, BuildSteps, CanisterInstructions, CanisterManifest, CanisterSettings, SyncSteps,
    };
    use icp_network::{BindPort, NETWORK_LOCAL, NetworkConfig};

    use crate::directory::ProjectDirectory;
    use crate::project::{LoadProjectManifestError, Project};

    #[test]
    fn empty_project() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Write project-manifest
        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            "",                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd).expect("failed to load project manifest");

        // Verify no canisters were found
        assert!(pm.canisters.is_empty());
    }

    #[test]
    fn single_canister_project() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Write project-manifest
        let pm = r#"
        canister:
          name: canister-1
          build:
            steps:
              - type: script
                command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#;

        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            pm,                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd).expect("failed to load project manifest");

        // Verify canister was loaded
        let canisters = vec![(
            project_dir.path().to_owned(),
            CanisterManifest {
                name: "canister-1".into(),
                settings: CanisterSettings::default(),
                instructions: CanisterInstructions::BuildSync {
                    build: BuildSteps {
                        steps: vec![BuildStep::Script(ScriptAdapter {
                            command: CommandField::Command(
                                "sh -c 'cp {} \"$ICP_WASM_OUTPUT_PATH\"'".into(),
                            ),
                        })],
                    },
                    sync: SyncSteps::default(),
                },
            },
        )];

        assert_eq!(pm.canisters, canisters);
    }

    #[test]
    fn multi_canister_project() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Create canister directory
        std::fs::create_dir(project_dir.path().join("canister-1"))
            .expect("failed to create canister directory");

        // Write canister-manifest
        let cm = r#"
        name: canister-1
        build:
          steps:
            - type: script
              command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
        "#;

        std::fs::write(
            project_dir.path().join("canister-1/canister.yaml"), // path
            cm,                                                  // contents
        )
        .expect("failed to write canister manifest");

        // Write project-manifest
        let pm = r#"
        canisters:
          - canister-1
        "#;

        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            pm,                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd).expect("failed to load project manifest");

        // Verify canister was loaded
        let canisters = vec![(
            project_dir.path().join("canister-1"),
            CanisterManifest {
                name: "canister-1".into(),
                settings: CanisterSettings::default(),
                instructions: CanisterInstructions::BuildSync {
                    build: BuildSteps {
                        steps: vec![BuildStep::Script(ScriptAdapter {
                            command: CommandField::Command(
                                "sh -c 'cp {} \"$ICP_WASM_OUTPUT_PATH\"'".into(),
                            ),
                        })],
                    },
                    sync: SyncSteps::default(),
                },
            },
        )];

        assert_eq!(pm.canisters, canisters);
    }

    #[test]
    fn invalid_project_manifest() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Write project-manifest
        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            "invalid-content",                   // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd);

        // Assert failure
        assert!(matches!(pm, Err(LoadProjectManifestError::Parse { .. })));
    }

    #[test]
    fn invalid_canister_manifest() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Create canister directory
        std::fs::create_dir(project_dir.path().join("canister-1"))
            .expect("failed to create canister directory");

        // Write canister-manifest
        std::fs::write(
            project_dir.path().join("canister-1/canister.yaml"), // path
            "invalid-content",                                   // contents
        )
        .expect("failed to write canister manifest");

        // Write project-manifest
        let pm = r#"
        canisters:
          - canister-1
        "#;

        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            pm,                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd);

        // Assert failure
        assert!(matches!(
            pm,
            Err(LoadProjectManifestError::CanisterLoad { .. })
        ));
    }

    #[test]
    fn glob_path_non_canister() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Create canister directory
        std::fs::create_dir_all(project_dir.path().join("canisters/canister-1"))
            .expect("failed to create canister directory");

        // Skip writing canister-manifest
        //

        // Write project-manifest
        let pm = r#"
        canisters:
          - canisters/*
        "#;

        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            pm,                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd).expect("failed to load project manifest");

        // Verify no canisters were found
        assert!(pm.canisters.is_empty());
    }

    #[test]
    fn explicit_path_non_canister() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Create canister directory
        std::fs::create_dir(project_dir.path().join("canister-1"))
            .expect("failed to create canister directory");

        // Skip writing canister-manifest
        //

        // Write project-manifest
        let pm = r#"
        canisters:
          - canister-1
        "#;

        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            pm,                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd);

        // Assert failure
        assert!(matches!(
            pm,
            Err(LoadProjectManifestError::NoManifest { .. })
        ));
    }

    #[test]
    fn invalid_glob_pattern() {
        // Setup
        let project_dir = tempdir().expect("failed to create temporary project directory");

        // Write project-manifest
        let pm = r#"
        canisters:
          - canisters/***
        "#;

        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            pm,                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd);

        // Assert failure
        assert!(matches!(
            pm,
            Err(LoadProjectManifestError::GlobPattern { .. })
        ));
    }

    #[test]
    fn default_local_network() {
        let project_dir = tempdir().expect("failed to create temporary project directory");

        let pm = r#""#;

        std::fs::write(
            project_dir.path().join("icp.yaml"), // path
            pm,                                  // contents
        )
        .expect("failed to write project manifest");

        // Load Project
        let pd = ProjectDirectory::new(project_dir.path());
        let pm = Project::load(pd).unwrap();

        let local_network = pm
            .get_network_config(NETWORK_LOCAL)
            .expect("local network should be defined");

        let NetworkConfig::Managed(managed) = local_network else {
            panic!("Expected local network to be managed");
        };

        // Check that the local network has the default configuration
        assert_eq!(managed.gateway.host, "127.0.0.1");
        assert!(matches!(managed.gateway.port, BindPort::Fixed(8000)));
    }
}
