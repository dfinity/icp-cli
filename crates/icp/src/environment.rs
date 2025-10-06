use async_trait::async_trait;

use crate::{Environment, LoadManifest, manifest::EnvironmentManifest};

#[derive(Debug, thiserror::Error)]
pub enum LoadManifestError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub struct Loader;

#[async_trait]
impl LoadManifest<EnvironmentManifest, Environment, LoadManifestError> for Loader {
    async fn load(&self, m: &EnvironmentManifest) -> Result<Environment, LoadManifestError> {
        Ok(Environment {
            name: m.name.to_owned(),
            network: todo!(),
            canisters: todo!(),
        })
    }
}
