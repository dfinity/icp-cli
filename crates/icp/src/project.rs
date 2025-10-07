use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
    vec,
};

use anyhow::Context;
use async_trait::async_trait;

use crate::{
    Canister, Environment, LoadManifest, LoadPath, Network, Project,
    canister::{self, recipe},
    fs::read,
    is_glob,
    manifest::{
        CANISTER_MANIFEST, CanisterManifest, Item, Locate, PROJECT_MANIFEST,
        canister::Instructions, environment::CanisterSelection, project::ProjectManifest,
    },
    prelude::*,
};

#[derive(Debug, thiserror::Error)]
pub enum LoadPathError {
    #[error("failed to read canister manifest")]
    Read,

    #[error("failed to deserialize canister manifest")]
    Deserialize,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub struct PathLoader;

#[async_trait]
impl LoadPath<ProjectManifest, LoadPathError> for PathLoader {
    async fn load(&self, path: &Path) -> Result<ProjectManifest, LoadPathError> {
        // Read file
        let mbs = read(&path.join(PROJECT_MANIFEST)).context(LoadPathError::Read)?;

        // Load YAML
        let m =
            serde_yaml::from_slice::<ProjectManifest>(&mbs).context(LoadPathError::Deserialize)?;

        Ok(m)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnvironmentError {
    #[error("environment '{environment}' points to invalid network '{network}'")]
    Network {
        environment: String,
        network: String,
    },

    #[error("environment '{environment}' points to invalid canister '{canister}'")]
    Canister {
        environment: String,
        canister: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum LoadManifestError {
    #[error("failed to locate project directory")]
    Locate,

    #[error("failed to perform glob parsing")]
    Glob,

    #[error("failed to load canister manifest")]
    Canister,

    #[error("failed to resolve canister recipe")]
    Recipe,

    #[error("project contains two similarly named {kind}s: '{name}'")]
    Duplicate { kind: String, name: String },

    #[error(transparent)]
    Environment(#[from] EnvironmentError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub struct ManifestLoader {
    pub locate: Arc<dyn Locate>,
    pub recipe: Arc<dyn recipe::Resolve>,
    pub canister: Arc<dyn LoadPath<CanisterManifest, canister::LoadPathError>>,
}

#[async_trait]
impl LoadManifest<ProjectManifest, Project, LoadManifestError> for ManifestLoader {
    async fn load(&self, m: &ProjectManifest) -> Result<Project, LoadManifestError> {
        // Locate project root
        let pdir = self.locate.locate().context(LoadManifestError::Locate)?;

        // Canisters
        let mut canisters: HashMap<String, (PathBuf, Canister)> = HashMap::new();

        for i in &m.canisters {
            let ms = match i {
                Item::Path(pattern) => {
                    let paths = match is_glob(pattern) {
                        // Explicit path
                        false => vec![pdir.join(pattern)],

                        // Glob pattern
                        true => {
                            // Resolve glob
                            let paths = glob::glob(pdir.join(pattern).as_str())
                                .context(LoadManifestError::Glob)?;

                            // Extract paths
                            let paths = paths
                                .collect::<Result<Vec<_>, _>>()
                                .context(LoadManifestError::Glob)?;

                            // Convert to utf-8
                            paths
                                .into_iter()
                                .map(PathBuf::try_from)
                                .collect::<Result<Vec<_>, _>>()
                                .context(LoadManifestError::Glob)?
                        }
                    };

                    let paths = paths
                        .into_iter()
                        .filter(|p| p.is_dir()) // Skip missing directories
                        .filter(|p| p.join(CANISTER_MANIFEST).exists()) // Skip non-canister directories
                        .collect::<Vec<_>>();

                    let mut ms = vec![];

                    for p in paths {
                        ms.push((
                            //
                            // Canister root
                            p.to_owned(),
                            //
                            // Canister manifest
                            self.canister
                                .load(&p)
                                .await
                                .context(LoadManifestError::Canister)?,
                        ));
                    }

                    ms
                }

                Item::Manifest(m) => vec![(
                    //
                    // Caniser root
                    pdir.to_owned(),
                    //
                    // Canister manifest
                    m.to_owned(),
                )],
            };

            for (cdir, m) in ms {
                let (build, sync) = match &m.instructions {
                    // Build/Sync
                    Instructions::BuildSync { build, sync } => (build.to_owned(), sync.to_owned()),

                    // Recipe
                    Instructions::Recipe { recipe } => self
                        .recipe
                        .resolve(recipe)
                        .await
                        .context(LoadManifestError::Recipe)?,
                };

                // Check for duplicates
                match canisters.entry(m.name.to_owned()) {
                    // Duplicate
                    Entry::Occupied(e) => {
                        return Err(LoadManifestError::Duplicate {
                            kind: "canister".to_string(),
                            name: e.key().to_owned(),
                        });
                    }

                    // Ok
                    Entry::Vacant(e) => {
                        e.insert((
                            //
                            // Caniser root
                            cdir,
                            //
                            // Canister
                            Canister {
                                name: m.name.to_owned(),
                                settings: m.settings.to_owned(),
                                build,
                                sync,
                            },
                        ));
                    }
                }
            }
        }

        // Networks
        let mut networks: HashMap<String, Network> = HashMap::new();

        for m in &m.networks {
            match networks.entry(m.name.to_owned()) {
                // Duplicate
                Entry::Occupied(e) => {
                    return Err(LoadManifestError::Duplicate {
                        kind: "network".to_string(),
                        name: e.key().to_owned(),
                    });
                }

                // Ok
                Entry::Vacant(e) => {
                    e.insert(Network {
                        name: m.name.to_owned(),
                        configuration: m.configuration.to_owned(),
                    });
                }
            }
        }

        // Environments
        let mut environments: HashMap<String, Environment> = HashMap::new();

        for m in &m.environments {
            match environments.entry(m.name.to_owned()) {
                // Duplicate
                Entry::Occupied(e) => {
                    return Err(LoadManifestError::Duplicate {
                        kind: "environment".to_string(),
                        name: e.key().to_owned(),
                    });
                }

                // Ok
                Entry::Vacant(e) => {
                    e.insert(Environment {
                        name: m.name.to_owned(),

                        // Embed network in environment
                        network: {
                            let v = networks.get(&m.network).ok_or(EnvironmentError::Network {
                                environment: m.name.to_owned(),
                                network: m.network.to_owned(),
                            })?;

                            v.to_owned()
                        },

                        // Embed canisters in environment
                        canisters: {
                            match &m.canisters {
                                // None
                                CanisterSelection::None => HashMap::new(),

                                // Everything
                                CanisterSelection::Everything => canisters.clone(),

                                // Named
                                CanisterSelection::Named(names) => {
                                    let mut canisters: HashMap<String, (PathBuf, Canister)> =
                                        HashMap::new();

                                    for name in names {
                                        let v = canisters.get(name).ok_or(
                                            EnvironmentError::Canister {
                                                environment: m.name.to_owned(),
                                                canister: name.to_owned(),
                                            },
                                        )?;

                                        canisters.insert(name.to_owned(), v.to_owned());
                                    }

                                    canisters
                                }
                            }
                        },
                    });
                }
            }
        }

        Ok(Project {
            canisters,
            networks,
            environments,
        })
    }
}

// #[cfg(test)]
// mod tests {
//     use camino_tempfile::tempdir;
//     use icp_adapter::script::{CommandField, ScriptAdapter};
//     use icp_network::{BindPort, NETWORK_LOCAL, NetworkConfig};

//     use crate::{Canister, LoadProjectManifestError, Project, directory::ProjectDirectory};

//     #[tokio::test]
//     async fn empty_project() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Write project-manifest
//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             "",                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None)
//             .await
//             .expect("failed to load project manifest");

//         // Verify no canisters were found
//         assert!(pm.canisters.is_empty());
//     }

//     #[tokio::test]
//     async fn single_canister_project() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Write project-manifest
//         let pm = r#"
//         canister:
//           name: canister-1
//           build:
//             steps:
//               - type: script
//                 command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
//         "#;

//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             pm,                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None)
//             .await
//             .expect("failed to load project manifest");

//         // Verify canister was loaded
//         let canisters = vec![(
//             project_dir.path().to_owned(),
//             Canister {
//                 name: "canister-1".into(),
//                 settings: Settings::default(),
//                 build: BuildSteps {
//                     steps: vec![BuildStep::Script(ScriptAdapter {
//                         command: CommandField::Command(
//                             "sh -c 'cp {} \"$ICP_WASM_OUTPUT_PATH\"'".into(),
//                         ),
//                         stdio_sender: None,
//                     })],
//                 },
//                 sync: SyncSteps::default(),
//             },
//         )];

//         assert_eq!(pm.canisters, canisters);
//     }

//     #[tokio::test]
//     async fn multi_canister_project() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Create canister directory
//         std::fs::create_dir(project_dir.path().join("canister-1"))
//             .expect("failed to create canister directory");

//         // Write canister-manifest
//         let cm = r#"
//         name: canister-1
//         build:
//           steps:
//             - type: script
//               command: sh -c 'cp {} "$ICP_WASM_OUTPUT_PATH"'
//         "#;

//         std::fs::write(
//             project_dir.path().join("canister-1/canister.yaml"), // path
//             cm,                                                  // contents
//         )
//         .expect("failed to write canister manifest");

//         // Write project-manifest
//         let pm = r#"
//         canisters:
//           - canister-1
//         "#;

//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             pm,                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None)
//             .await
//             .expect("failed to load project manifest");

//         // Verify canister was loaded
//         let canisters = vec![(
//             project_dir.path().join("canister-1"),
//             Canister {
//                 name: "canister-1".into(),
//                 settings: Settings::default(),
//                 build: BuildSteps {
//                     steps: vec![BuildStep::Script(ScriptAdapter {
//                         command: CommandField::Command(
//                             "sh -c 'cp {} \"$ICP_WASM_OUTPUT_PATH\"'".into(),
//                         ),
//                         stdio_sender: None,
//                     })],
//                 },
//                 sync: SyncSteps::default(),
//             },
//         )];

//         assert_eq!(pm.canisters, canisters);
//     }

//     #[tokio::test]
//     async fn invalid_project_manifest() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Write project-manifest
//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             "invalid-content",                   // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None).await;

//         // Assert failure
//         assert!(matches!(pm, Err(LoadProjectManifestError::Parse { .. })));
//     }

//     #[tokio::test]
//     async fn invalid_canister_manifest() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Create canister directory
//         std::fs::create_dir(project_dir.path().join("canister-1"))
//             .expect("failed to create canister directory");

//         // Write canister-manifest
//         std::fs::write(
//             project_dir.path().join("canister-1/canister.yaml"), // path
//             "invalid-content",                                   // contents
//         )
//         .expect("failed to write canister manifest");

//         // Write project-manifest
//         let pm = r#"
//         canisters:
//           - canister-1
//         "#;

//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             pm,                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None).await;

//         // Assert failure
//         assert!(matches!(
//             pm,
//             Err(LoadProjectManifestError::CanisterLoad { .. })
//         ));
//     }

//     #[tokio::test]
//     async fn glob_path_non_canister() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Create canister directory
//         std::fs::create_dir_all(project_dir.path().join("canisters/canister-1"))
//             .expect("failed to create canister directory");

//         // Skip writing canister-manifest
//         //

//         // Write project-manifest
//         let pm = r#"
//         canisters:
//           - canisters/*
//         "#;

//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             pm,                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None)
//             .await
//             .expect("failed to load project manifest");

//         // Verify no canisters were found
//         assert!(pm.canisters.is_empty());
//     }

//     #[tokio::test]
//     async fn explicit_path_non_canister() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Create canister directory
//         std::fs::create_dir(project_dir.path().join("canister-1"))
//             .expect("failed to create canister directory");

//         // Skip writing canister-manifest
//         //

//         // Write project-manifest
//         let pm = r#"
//         canisters:
//           - canister-1
//         "#;

//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             pm,                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None).await;

//         // Assert failure
//         assert!(matches!(
//             pm,
//             Err(LoadProjectManifestError::NoManifest { .. })
//         ));
//     }

//     #[tokio::test]
//     async fn invalid_glob_pattern() {
//         // Setup
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         // Write project-manifest
//         let pm = r#"
//         canisters:
//           - canisters/***
//         "#;

//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             pm,                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None).await;

//         // Assert failure
//         assert!(matches!(
//             pm,
//             Err(LoadProjectManifestError::GlobPattern { .. })
//         ));
//     }

//     #[tokio::test]
//     async fn default_local_network() {
//         let project_dir = tempdir().expect("failed to create temporary project directory");

//         let pm = r#""#;

//         std::fs::write(
//             project_dir.path().join("icp.yaml"), // path
//             pm,                                  // contents
//         )
//         .expect("failed to write project manifest");

//         // Load Project
//         let pd = ProjectDirectory::new(project_dir.path());
//         let pm = Project::load(pd, None).await.unwrap();

//         let local_network = pm
//             .get_network_config(NETWORK_LOCAL)
//             .expect("local network should be defined");

//         let NetworkConfig::Managed(managed) = local_network else {
//             panic!("Expected local network to be managed");
//         };

//         // Check that the local network has the default configuration
//         assert_eq!(managed.gateway.host, "127.0.0.1");
//         assert!(matches!(managed.gateway.port, BindPort::Fixed(8000)));
//     }
// }
