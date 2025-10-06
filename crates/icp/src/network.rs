use async_trait::async_trait;

use crate::{LoadManifest, Network, manifest::NetworkManifest};

#[derive(Debug, thiserror::Error)]
pub enum LoadManifestError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub struct Loader;

#[async_trait]
impl LoadManifest<NetworkManifest, Network, LoadManifestError> for Loader {
    async fn load(&self, m: &NetworkManifest) -> Result<Network, LoadManifestError> {
        Ok(Network {
            name: m.name.to_owned(),
        })
    }
}
