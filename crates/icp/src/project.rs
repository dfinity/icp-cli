use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;

use crate::{
    Canister, Environment, LoadManifest, LoadPath, Network, Project,
    canister::{self, recipe},
    fs::read,
    is_glob,
    manifest::{
        CANISTER_MANIFEST, CanisterManifest, Item, Locate, PROJECT_MANIFEST,
        canister::Instructions, project::ProjectManifest,
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
pub enum LoadManifestError {
    #[error("failed to locate project directory")]
    Locate,

    #[error("failed to perform glob parsing")]
    Glob,

    #[error("failed to load canister manifest")]
    Canister,

    #[error("failed to resolve canister recipe")]
    Recipe,

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
        let mut canisters = vec![];

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
                        ms.push(
                            self.canister
                                .load(&p)
                                .await
                                .context(LoadManifestError::Canister)?,
                        );
                    }

                    ms
                }

                Item::Manifest(m) => vec![m.to_owned()],
            };

            for m in ms {
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

                canisters.push(Canister {
                    name: m.name.to_owned(),
                    settings: m.settings.to_owned(),
                    build,
                    sync,
                });
            }
        }

        // Networks
        let mut networks = vec![];

        for m in &m.networks {
            networks.push(Network {
                name: m.name.to_owned(),
            });
        }

        // Environments
        let mut environments = vec![];

        for m in &m.environments {
            environments.push(Environment {
                name: m.name.to_owned(),
                network: todo!(),
                canisters: todo!(),
            });
        }

        Ok(Project {
            canisters,
            networks,
            environments,
        })
    }
}
