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
        CANISTER_MANIFEST, CanisterManifest, Item, Locate, canister::Instructions,
        environment::CanisterSelection, project::ProjectManifest, recipe::RecipeType,
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
        let mbs = read(path).context(LoadPathError::Read)?;

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

    #[error("failed to resolve canister recipe: {0}")]
    Recipe(RecipeType),

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
                    Instructions::BuildSync { build, sync } => (build.to_owned(), sync.to_owned()),

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
