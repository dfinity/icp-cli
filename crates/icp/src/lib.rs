use std::sync::Arc;

use async_trait::async_trait;
pub use directories::{Directories, DirectoriesError};
use schemars::JsonSchema;

use crate::{
    canister::{Settings, build, sync},
    manifest::Item,
};

pub mod canister;
mod directories;
pub mod fs;
pub mod manifest;
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

pub struct Network {
    name: String,
}

pub struct Environment {
    name: String,
    network: Network,
    canisters: Vec<Canister>,
}

pub struct Project {
    pub canisters: Vec<Canister>,
    pub networks: Vec<Network>,
    pub environments: Vec<Environment>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to load manifest: {0}")]
    Manifest(#[from] manifest::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load {
    async fn load(&self) -> Result<Project, LoadError>;
}

pub struct Loader {
    manifest: Arc<dyn manifest::Load>,
    // recipe: Arc<dyn
}

#[async_trait]
impl Load for Loader {
    async fn load(&self) -> Result<Project, LoadError> {
        // Load manifest
        let m = self.manifest.load()?;

        // Canisters
        let canisters: Vec<_> = m
            .canisters
            .into_iter()
            .map(|v| match v {
                Item::Path(p) => todo!(),
                Item::Manifest(m) => todo!(),
            })
            .collect();

        // Networks
        let networks: Vec<_> = m
            .networks
            .into_iter()
            .map(|v| match v {
                Item::Path(p) => todo!(),
                Item::Manifest(m) => todo!(),
            })
            .collect();

        // Environments
        let environments: Vec<_> = m.environments.into_iter().map(|v| todo!()).collect();

        Ok(Project {
            canisters,
            networks,
            environments,
        })
    }
}
