use std::sync::Arc;

use async_trait::async_trait;
pub use directories::{Directories, DirectoriesError};
use schemars::JsonSchema;
use tokio::sync::Mutex;

use crate::{
    canister::{Settings, build, sync},
    fs::read,
    manifest::Item,
};

pub mod canister;
mod directories;
pub mod environment;
pub mod fs;
pub mod manifest;
pub mod network;
pub mod prelude;

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
    #[error("failed to load manifest: {0}")]
    Manifest(#[from] manifest::LoadError),

    #[error("failed to load canister: {0}")]
    Canister(#[from] canister::LoadError),

    #[error("failed to load network: {0}")]
    Network(#[from] network::LoadError),

    #[error("failed to load environment: {0}")]
    Environment(#[from] environment::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self) -> Result<Project, LoadError>;
}

pub struct Loader {
    manifest: Arc<dyn manifest::Load>,
    canister: Arc<dyn canister::Load>,
    network: Arc<dyn network::Load>,
    environment: Arc<dyn environment::Load>,
}

impl Loader {
    pub fn new(
        manifest: Arc<dyn manifest::Load>,
        canister: Arc<dyn canister::Load>,
        network: Arc<dyn network::Load>,
        environment: Arc<dyn environment::Load>,
    ) -> Self {
        Self {
            manifest,
            canister,
            network,
            environment,
        }
    }
}

#[async_trait]
impl Load for Loader {
    async fn load(&self) -> Result<Project, LoadError> {
        // Load manifest
        let m = self.manifest.load()?;

        // Canisters
        let mut canisters = vec![];

        for i in m.canisters {
            let m = match i {
                Item::Path(p) => {
                    // // Read file
                    // let bs = read(&p.join("icp.yaml"))?;

                    // // Load YAML
                    // let m = serde_yaml::from_slice::<manifest::Canister>(&bs)?;

                    // m

                    todo!()
                }

                Item::Manifest(m) => m,
            };

            canisters.push(self.canister.load(m).await?);
        }

        // Networks
        let mut networks = vec![];

        for i in m.networks {
            networks.push(match i {
                Item::Path(p) => todo!(),
                Item::Manifest(m) => self.network.load(m).await?,
            });
        }

        // Environments
        let mut environments = vec![];

        for m in m.environments {
            environments.push(self.environment.load(m).await?);
        }

        let environments: Vec<_> = m.environments.into_iter().map(|v| todo!()).collect();

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
