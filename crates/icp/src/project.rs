use std::{
    collections::{HashMap, hash_map::Entry},
    sync::Arc,
    vec,
};

use anyhow::Context;
use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::{
    canister::{self, recipe, sync::Steps}, fs::read, is_glob, manifest::{
        canister::Instructions, environment::CanisterSelection, project::ProjectManifest, recipe::RecipeType, CanisterManifest, Item, Locate, CANISTER_MANIFEST
    }, network::Configuration, prelude::*, Canister, Environment, LoadManifest, LoadPath, Network, Project
};

#[derive(Debug, thiserror::Error)]
pub enum LoadPathError {
    #[error("failed to read manifest at {0}")]
    Read(String),

    #[error("failed to deserialize manifest at {0}")]
    Deserialize(String),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub struct PathLoader;

#[async_trait]
impl<T> LoadPath<T, LoadPathError> for PathLoader
where
    T: DeserializeOwned + Send + 'static,
{
    async fn load(&self, path: &Path) -> Result<T, LoadPathError> {
        // Read file
        let mbs = read(path).context(LoadPathError::Read(path.to_string()))?;

        // Deserialize YAML into any T
        let m = serde_yaml::from_slice::<T>(&mbs)
            .context(LoadPathError::Deserialize(path.to_string()))?;

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

    #[error("failed to load {kind} manifest at: {path}")]
    Failed { kind: String, path: String },

    #[error("failed to resolve canister recipe: {0}")]
    Recipe(RecipeType),

    #[error("project contains two similarly named {kind}s: '{name}'")]
    Duplicate { kind: String, name: String },

    #[error("Could not locate a {kind} manifest at: '{path}'")]
    NotFound { kind: String, path: String },

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

        let canister_manifests: Vec<Item<CanisterManifest>> = m.canisters.clone().into();

        for i in canister_manifests {
            let ms = match i {
                Item::Path(pattern) => {
                    let paths = match is_glob(&pattern) {
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
                                .load(&p.join(CANISTER_MANIFEST))
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
                    Instructions::BuildSync { build, sync } => 
                    (build.to_owned(),
                        match sync {
                            Some(sync) => sync.to_owned(),
                            None => Steps::default(),
                        }),

                    // Recipe
                    Instructions::Recipe { recipe } => self
                        .recipe
                        .resolve(recipe)
                        .await
                        .context(LoadManifestError::Recipe(recipe.recipe_type.clone()))?,
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

        for i in &m.networks {
            let m = match i {
                Item::Path(path) => {
                    let path = pdir.join(path);
                    if !path.exists() || !path.is_file() {
                        return Err(LoadManifestError::NotFound {
                            kind: "network".to_string(),
                            path: path.to_string(),
                        });
                    }
                    let loader = PathLoader;
                    loader
                        .load(&path)
                        .await
                        .context(LoadManifestError::Failed {
                            kind: "network".to_string(),
                            path: path.to_string(),
                        })?
                }
                Item::Manifest(ms) => ms.clone(),
            };

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
                        configuration: Configuration::default(),
                    });
                }
            }
        }

        // Environments
        let mut environments: HashMap<String, Environment> = HashMap::new();

        for i in &m.environments {
            let m = match i {
                Item::Path(path) => {
                    let path = pdir.join(path);
                    if !path.exists() || !path.is_file() {
                        return Err(LoadManifestError::NotFound {
                            kind: "environment".to_string(),
                            path: path.to_string(),
                        });
                    }
                    let loader = PathLoader;
                    loader
                        .load(&path)
                        .await
                        .context(LoadManifestError::Failed {
                            kind: "environment".to_string(),
                            path: path.to_string(),
                        })?
                }
                Item::Manifest(ms) => ms.clone(),
            };

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
                                    let mut cs: HashMap<String, (PathBuf, Canister)> =
                                        HashMap::new();

                                    for name in names {
                                        let v = canisters.get(name).ok_or(
                                            EnvironmentError::Canister {
                                                environment: m.name.to_owned(),
                                                canister: name.to_owned(),
                                            },
                                        )?;

                                        cs.insert(name.to_owned(), v.to_owned());
                                    }

                                    cs
                                }
                            }
                        },
                    });
                }
            }
        }

        Ok(Project {
            dir: pdir,
            canisters,
            networks,
            environments,
        })
    }
}
