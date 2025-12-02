use std::sync::Arc;

use async_trait::async_trait;
use ic_agent::Identity;
use snafu::prelude::*;

use crate::{
    fs::lock::{DirectoryStructureLock, LockError, PathsAccess},
    identity::{
        key::{
            LoadIdentityError, LoadIdentityInContextError, load_identity, load_identity_in_context,
        },
        manifest::{IdentityList, LoadIdentityManifestError},
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

pub struct IdentityPaths {
    dir: PathBuf,
}

impl IdentityPaths {
    pub fn new(dir: PathBuf) -> Result<IdentityDirectories, LockError> {
        DirectoryStructureLock::open_or_create(Self { dir })
    }

    pub fn identity_defaults_path(&self) -> PathBuf {
        self.dir.join(IDENTITY_DEFAULTS)
    }

    pub fn ensure_identity_defaults_path(&self) -> Result<PathBuf, crate::fs::IoError> {
        crate::fs::create_dir_all(&self.dir)?;
        Ok(self.dir.join(IDENTITY_DEFAULTS))
    }

    pub fn identity_list_path(&self) -> PathBuf {
        self.dir.join(IDENTITIES_LIST)
    }

    pub fn ensure_identity_list_path(&self) -> Result<PathBuf, crate::fs::IoError> {
        crate::fs::create_dir_all(&self.dir)?;
        Ok(self.dir.join(IDENTITIES_LIST))
    }

    pub fn key_pem_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("keys/{name}.pem"))
    }

    pub fn ensure_key_pem_path(&self, name: &str) -> Result<PathBuf, crate::fs::IoError> {
        crate::fs::create_dir_all(&self.dir.join("keys"))?;
        Ok(self.dir.join(format!("keys/{name}.pem")))
    }
}

pub type IdentityDirectories = DirectoryStructureLock<IdentityPaths>;

impl PathsAccess for IdentityPaths {
    fn lock_file(&self) -> PathBuf {
        self.dir.join(".lock")
    }
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

#[derive(Debug, Snafu)]
pub enum LoadError {
    #[snafu(transparent)]
    LoadIdentityInContext { source: LoadIdentityInContextError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityError },

    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    LockIdentityDirError { source: LockError },

    #[snafu(transparent)]
    Unexpected { source: anyhow::Error },
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self, id: IdentitySelection) -> Result<Arc<dyn Identity>, LoadError>;
}

pub struct Loader {
    pub dir: IdentityDirectories,
}

#[async_trait]
impl Load for Loader {
    async fn load(&self, id: IdentitySelection) -> Result<Arc<dyn Identity>, LoadError> {
        match id {
            IdentitySelection::Default => Ok(self
                .dir
                .with_read(async |dirs| load_identity_in_context(dirs, || unimplemented!()).await)
                .await??),

            IdentitySelection::Anonymous => {
                self.dir
                    .with_read(async |dirs| {
                        Ok(load_identity(
                            dirs,
                            &IdentityList::load_from(dirs)?,
                            "anonymous",
                            || unimplemented!(),
                        )?)
                    })
                    .await?
            }

            IdentitySelection::Named(name) => {
                self.dir
                    .with_read(async |dirs| {
                        Ok(load_identity(
                            dirs,
                            &IdentityList::load_from(dirs)?,
                            &name,
                            || unimplemented!(),
                        )?)
                    })
                    .await?
            }
        }
    }
}

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
pub struct MockIdentityLoader {
    /// The default identity to return when IdentitySelection::Default is used
    default: Arc<dyn Identity>,

    /// Named identities that can be selected
    named: HashMap<String, Arc<dyn Identity>>,
}

#[cfg(test)]
impl MockIdentityLoader {
    /// Creates a new mock identity loader with the given default identity.
    pub fn new(default: Arc<dyn Identity>) -> Self {
        Self {
            default,
            named: HashMap::new(),
        }
    }

    /// Creates a mock identity loader with anonymous as the default.
    pub fn anonymous() -> Self {
        Self::new(Arc::new(ic_agent::identity::AnonymousIdentity))
    }

    /// Adds a named identity to the loader.
    pub fn with_identity(mut self, name: impl Into<String>, identity: Arc<dyn Identity>) -> Self {
        self.named.insert(name.into(), identity);
        self
    }

    /// Sets the default identity.
    pub fn with_default(mut self, identity: Arc<dyn Identity>) -> Self {
        self.default = identity;
        self
    }
}

#[cfg(test)]
#[async_trait]
impl Load for MockIdentityLoader {
    async fn load(&self, id: IdentitySelection) -> Result<Arc<dyn Identity>, LoadError> {
        Ok(match id {
            IdentitySelection::Default => Arc::clone(&self.default),

            IdentitySelection::Anonymous => Arc::new(ic_agent::identity::AnonymousIdentity),

            IdentitySelection::Named(name) => {
                self.named
                    .get(&name)
                    .map(Arc::clone)
                    .ok_or_else(|| LoadError::LoadIdentity {
                        source: LoadIdentityError::NoSuchIdentity { name: name.clone() },
                    })?
            }
        })
    }
}
