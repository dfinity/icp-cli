use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;

use crate::{
    CanisterLoaders, Environment, LoadManifest, LoadPath, Network, Project, environment,
    fs::read,
    is_glob,
    manifest::{
        CANISTER_MANIFEST, EnvironmentManifest, Item, Locate, NetworkManifest, PROJECT_MANIFEST,
        project::ProjectManifest,
    },
    network,
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

    #[error("failed to load network")]
    Network,

    #[error("failed to load environment")]
    Environment,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub struct ManifestLoader {
    locate: Arc<dyn Locate>,
    canister: CanisterLoaders,
    network: Arc<dyn LoadManifest<NetworkManifest, Network, network::LoadManifestError>>,
    environment:
        Arc<dyn LoadManifest<EnvironmentManifest, Environment, environment::LoadManifestError>>,
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
                                .path
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
                canisters.push(
                    self.canister
                        .manifest
                        .load(&m)
                        .await
                        .context(LoadManifestError::Canister)?,
                );
            }
        }

        // Networks
        let mut networks = vec![];

        for m in &m.networks {
            networks.push(
                self.network
                    .load(m)
                    .await
                    .context(LoadManifestError::Network)?,
            );
        }

        // Environments
        let mut environments = vec![];

        for m in &m.environments {
            environments.push(
                self.environment
                    .load(m)
                    .await
                    .context(LoadManifestError::Environment)?,
            );
        }

        Ok(Project {
            canisters,
            networks,
            environments,
        })
    }
}
