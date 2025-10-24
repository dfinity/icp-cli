use std::sync::Arc;

use async_trait::async_trait;
use ic_agent::Identity;

use crate::{
    identity::{
        key::{
            LoadIdentityError, LoadIdentityInContextError, load_identity, load_identity_in_context,
        },
        manifest::{LoadIdentityManifestError, load_identity_list},
    },
    prelude::*,
};

pub mod key;
pub mod manifest;
pub mod seed;

/// Name of the default identities file
const IDENTITY_DEFAULTS: &str = "identity_defaults.json";

/// Name of the identities list file
const IDENTITIES_LIST: &str = "identity_list.json";

pub fn identity_defaults_path(dir: &Path) -> PathBuf {
    dir.join(IDENTITY_DEFAULTS)
}

pub fn ensure_identity_defaults_path(dir: &Path) -> Result<PathBuf, crate::fs::Error> {
    crate::fs::create_dir_all(dir)?;
    Ok(dir.join(IDENTITY_DEFAULTS))
}

pub fn identity_list_path(dir: &Path) -> PathBuf {
    dir.join(IDENTITIES_LIST)
}

pub fn ensure_identity_list_path(dir: &Path) -> Result<PathBuf, crate::fs::Error> {
    crate::fs::create_dir_all(dir)?;
    Ok(dir.join(IDENTITIES_LIST))
}

pub fn key_pem_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("keys/{name}.pem"))
}

pub fn ensure_key_pem_path(dir: &Path, name: &str) -> Result<PathBuf, crate::fs::Error> {
    crate::fs::create_dir_all(&dir.join("keys"))?;
    Ok(dir.join(format!("keys/{name}.pem")))
}

#[derive(Clone, Debug, PartialEq)]
pub enum IdentitySelection {
    /// Current default
    Default,

    /// Anonymous
    Anonymous,

    /// By name
    Named(String),
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    LoadIdentityInContext(#[from] LoadIdentityInContextError),

    #[error(transparent)]
    LoadIdentity(#[from] LoadIdentityError),

    #[error(transparent)]
    LoadIdentityManifest(#[from] LoadIdentityManifestError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self, id: IdentitySelection) -> Result<Arc<dyn Identity>, LoadError>;
}

pub struct Loader {
    pub dir: PathBuf,
}

#[async_trait]
impl Load for Loader {
    async fn load(&self, id: IdentitySelection) -> Result<Arc<dyn Identity>, LoadError> {
        Ok(match id {
            IdentitySelection::Default => load_identity_in_context(
                &self.dir,           // dir
                || unimplemented!(), // password_func
            )?,

            IdentitySelection::Anonymous => load_identity(
                &self.dir,                       // dir
                &load_identity_list(&self.dir)?, // list
                "anonymous",                     // name
                || unimplemented!(),             // password_func
            )?,

            IdentitySelection::Named(name) => load_identity(
                &self.dir,                       // dir
                &load_identity_list(&self.dir)?, // list
                &name,                           // name
                || unimplemented!(),             // password_func
            )?,
        })
    }
}
