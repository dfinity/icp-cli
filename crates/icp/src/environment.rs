use async_trait::async_trait;

use crate::{Environment, manifest};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self, m: manifest::Environment) -> Result<Environment, LoadError>;
}

pub struct Loader;

#[async_trait]
impl Load for Loader {
    async fn load(&self, m: manifest::Environment) -> Result<Environment, LoadError> {
        Ok(Environment {
            name: m.name,
            network: todo!(),
            canisters: todo!(),
        })
    }
}
