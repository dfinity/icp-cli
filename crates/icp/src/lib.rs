use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
pub use directories::{Directories, DirectoriesError};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::{
    canister::{Settings, build, sync},
    manifest::{PROJECT_MANIFEST, project::ProjectManifest},
    network::Configuration,
    prelude::*,
};

pub mod agent;
pub mod canister;
mod directories;
pub mod fs;
pub mod identity;
pub mod manifest;
pub mod network;
pub mod prelude;
pub mod project;

fn is_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Canister {
    pub name: String,

    /// Canister settings, such as memory constaints, etc.
    pub settings: Settings,

    /// The build configuration specifying how to compile the canister's source
    /// code into a WebAssembly module, including the adapter to use.
    pub build: build::Steps,

    /// The configuration specifying how to sync the canister
    pub sync: sync::Steps,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Network {
    pub name: String,
    pub configuration: Configuration,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Environment {
    pub name: String,
    pub network: Network,
    pub canisters: HashMap<String, (PathBuf, Canister)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Project {
    pub canisters: HashMap<String, (PathBuf, Canister)>,
    pub networks: HashMap<String, Network>,
    pub environments: HashMap<String, Environment>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to locate project directory")]
    Locate,

    #[error("failed to load path")]
    Path,

    #[error("failed to load the project manifest")]
    Manifest,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self, dir: &Path) -> Result<Project, LoadError>;
}

#[async_trait]
pub trait LoadPath<M, E>: Sync + Send {
    async fn load(&self, path: &Path) -> Result<M, E>;
}

#[async_trait]
pub trait LoadManifest<M, T, E>: Sync + Send {
    async fn load(&self, m: &M) -> Result<T, E>;
}

pub struct ProjectLoaders {
    pub path: Arc<dyn LoadPath<ProjectManifest, project::LoadPathError>>,
    pub manifest: Arc<dyn LoadManifest<ProjectManifest, Project, project::LoadManifestError>>,
}

pub struct Loader {
    project: ProjectLoaders,
}

impl Loader {
    pub fn new(project: ProjectLoaders) -> Self {
        Self { project }
    }
}

#[async_trait]
impl Load for Loader {
    async fn load(&self, dir: &Path) -> Result<Project, LoadError> {
        // Load project manifest
        let m = self
            .project
            .path
            .load(&dir.join(PROJECT_MANIFEST))
            .await
            .context(LoadError::Path)?;

        // Load project
        let p = self
            .project
            .manifest
            .load(&m)
            .await
            .context(LoadError::Manifest)?;

        Ok(p)
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
    async fn load(&self, dir: &Path) -> Result<Project, LoadError> {
        if let Some(v) = self.1.lock().await.as_ref() {
            return Ok(v.to_owned());
        }

        let v = self.0.load(dir).await?;

        let mut g = self.1.lock().await;
        if g.is_none() {
            *g = Some(v.to_owned());
        }

        Ok(v)
    }
}
