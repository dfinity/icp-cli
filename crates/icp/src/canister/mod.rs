use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    Canister, LoadManifest, LoadPath,
    fs::read,
    manifest::{CANISTER_MANIFEST, CanisterManifest, canister::Instructions},
    prelude::*,
};

pub mod build;
pub mod recipe;
pub mod sync;

/// Canister settings, such as compute and memory allocation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema)]
pub struct Settings {
    /// Compute allocation (0 to 100). Represents guaranteed compute capacity.
    pub compute_allocation: Option<u64>,

    /// Memory allocation in bytes. If unset, memory is allocated dynamically.
    pub memory_allocation: Option<u64>,

    /// Freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    pub freezing_threshold: Option<u64>,

    /// Reserved cycles limit. If set, the canister cannot consume more than this many cycles.
    pub reserved_cycles_limit: Option<u64>,

    /// Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    pub wasm_memory_limit: Option<u64>,

    /// Wasm memory threshold in bytes. Triggers a callback when exceeded.
    pub wasm_memory_threshold: Option<u64>,

    /// Environment variables for the canister as key-value pairs.
    /// These variables are accessible within the canister and can be used to configure
    /// behavior without hardcoding values in the WASM module.
    pub environment_variables: Option<HashMap<String, String>>,
}

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
impl LoadPath<CanisterManifest, LoadPathError> for PathLoader {
    async fn load(&self, path: &Path) -> Result<CanisterManifest, LoadPathError> {
        // Read file
        let mbs = read(&path.join(CANISTER_MANIFEST)).context(LoadPathError::Read)?;

        // Load YAML
        let m =
            serde_yaml::from_slice::<CanisterManifest>(&mbs).context(LoadPathError::Deserialize)?;

        Ok(m)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadManifestError {
    #[error("failed to resolve recipe: {0}")]
    Recipe(#[from] recipe::ResolveError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub struct ManifestLoader {
    recipe: Arc<dyn recipe::Resolve>,
}

impl ManifestLoader {
    pub fn new(recipe: Arc<dyn recipe::Resolve>) -> Self {
        Self { recipe }
    }
}

#[async_trait]
impl LoadManifest<CanisterManifest, Canister, LoadManifestError> for ManifestLoader {
    async fn load(&self, m: &CanisterManifest) -> Result<Canister, LoadManifestError> {
        let (build, sync) = match &m.instructions {
            Instructions::Recipe { recipe } => self.recipe.resolve(recipe).await?,
            Instructions::BuildSync { build, sync } => (build.to_owned(), sync.to_owned()),
        };

        Ok(Canister {
            name: m.name.to_owned(),
            settings: m.settings.to_owned(),
            build,
            sync,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Build: Sync + Send {
    async fn build(step: build::Step) -> Result<(), BuildError>;
}

pub struct Builder;

#[async_trait]
impl Build for Builder {
    async fn build(step: build::Step) -> Result<(), BuildError> {
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Synchronize: Sync + Send {
    async fn sync(step: sync::Step) -> Result<(), SyncError>;
}

pub struct Syncer;

#[async_trait]
impl Synchronize for Syncer {
    async fn sync(step: sync::Step) -> Result<(), SyncError> {
        Ok(())
    }
}
