use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
pub use directories::{Directories, DirectoriesError};
use schemars::JsonSchema;
use tokio::sync::Mutex;

use crate::{
    canister::{Settings, build, sync},
    manifest::{CANISTER_MANIFEST, CanisterManifest, Item, Locate},
    prelude::*,
};

pub mod canister;
mod directories;
pub mod environment;
pub mod fs;
pub mod manifest;
pub mod network;
pub mod prelude;

fn is_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct Canister {
    pub name: String,

    #[serde(default)]
    pub settings: Settings,

    /// The build configuration specifying how to compile the canister's source
    /// code into a WebAssembly module, including the adapter to use.
    build: build::Steps,

    /// The configuration specifying how to sync the canister
    sync: sync::Steps,
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct Network {
    name: String,
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct Environment {
    name: String,
    network: Network,
    canisters: Vec<Canister>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct Project {
    pub canisters: Vec<Canister>,
    pub networks: Vec<Network>,
    pub environments: Vec<Environment>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to locate project directory")]
    Locate,

    #[error("failed to perform glob parsing")]
    Glob,

    #[error("failed to load manifest: {0}")]
    Manifest(#[from] manifest::LoadError),

    #[error("failed to load canister manifest")]
    Canister,

    #[error("failed to load network")]
    Network,

    #[error("failed to load environment")]
    Environment,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self) -> Result<Project, LoadError>;
}

#[async_trait]
pub trait LoadPath<M, E>: Sync + Send {
    async fn load(&self, path: &Path) -> Result<M, E>;
}

#[async_trait]
pub trait LoadManifest<M, T, E>: Sync + Send {
    async fn load(&self, m: &M) -> Result<T, E>;
}

pub struct CanisterLoaders {
    path: Arc<dyn LoadPath<CanisterManifest, canister::LoadPathError>>,
    manifest: Arc<dyn LoadManifest<CanisterManifest, Canister, canister::LoadManifestError>>,
}

pub struct Loader {
    locate: Arc<dyn Locate>,
    manifest: Arc<dyn manifest::Load>,
    canister: CanisterLoaders,
    network: Arc<dyn network::Load>,
    environment: Arc<dyn environment::Load>,
}

#[async_trait]
impl Load for Loader {
    async fn load(&self) -> Result<Project, LoadError> {
        // Locate project root
        let pdir = self.locate.locate().context(LoadError::Locate)?;

        // Load manifest
        let m = self.manifest.load()?;

        // Canisters
        let mut canisters = vec![];

        for i in m.canisters {
            let ms = match i {
                Item::Path(pattern) => {
                    let paths = match is_glob(&pattern) {
                        // Explicit path
                        false => vec![pdir.join(pattern)],

                        // Glob pattern
                        true => {
                            // Resolve glob
                            let paths =
                                glob::glob(pdir.join(pattern).as_str()).context(LoadError::Glob)?;

                            // Extract paths
                            let paths = paths
                                .collect::<Result<Vec<_>, _>>()
                                .context(LoadError::Glob)?;

                            // Convert to utf-8
                            paths
                                .into_iter()
                                .map(PathBuf::try_from)
                                .collect::<Result<Vec<_>, _>>()
                                .context(LoadError::Glob)?
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
                                .context(LoadError::Canister)?,
                        );
                    }

                    ms
                }

                Item::Manifest(m) => vec![m],
            };

            for m in ms {
                canisters.push(
                    self.canister
                        .manifest
                        .load(&m)
                        .await
                        .context(LoadError::Canister)?,
                );
            }
        }

        // Networks
        let mut networks = vec![];

        for m in m.networks {
            networks.push(self.network.load(m).await.context(LoadError::Network)?);
        }

        // Environments
        let mut environments = vec![];

        for m in m.environments {
            environments.push(
                self.environment
                    .load(m)
                    .await
                    .context(LoadError::Environment)?,
            );
        }

        Ok(Project {
            canisters,
            networks,
            environments,
        })
    }
}

pub struct Lazy<T, V>(T, Arc<Mutex<Option<V>>>);

impl<T, V> Lazy<T, V> {
    pub fn new(v: T) -> Self {
        Self(v, Arc::new(Mutex::new(None)))
    }
}

#[async_trait]
impl<T: Load> Load for Lazy<T, Project> {
    async fn load(&self) -> Result<Project, LoadError> {
        if let Some(v) = self.1.lock().await.as_ref() {
            return Ok(v.to_owned());
        }

        let v = self.0.load().await?;

        let mut g = self.1.lock().await;
        if g.is_none() {
            *g = Some(v.to_owned());
        }

        Ok(v)
    }
}
