use async_trait::async_trait;

use crate::{Network, manifest};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self, m: manifest::Network) -> Result<Network, LoadError>;
}

pub struct Loader;

#[async_trait]
impl Load for Loader {
    async fn load(&self, m: manifest::Network) -> Result<Network, LoadError> {
        Ok(Network { name: m.name })
    }
}
