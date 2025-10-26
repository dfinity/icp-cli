use crate::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;

pub(crate) mod adapter;
pub(crate) mod canister;
pub(crate) mod environment;
pub(crate) mod network;
pub mod project;
pub(crate) mod recipe;
pub(crate) mod serde_helpers;

pub use {canister::CanisterManifest, environment::EnvironmentManifest, network::NetworkManifest};

pub const PROJECT_MANIFEST: &str = "icp.yaml";
pub const CANISTER_MANIFEST: &str = "canister.yaml";

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Item<T> {
    /// Path to a manifest
    Path(String),

    /// The manifest
    Manifest(T),
}

#[derive(Debug, thiserror::Error)]
pub enum LocateError {
    #[error("project manifest not found in {0}")]
    NotFound(PathBuf),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub trait Locate: Sync + Send {
    fn locate(&self) -> Result<PathBuf, LocateError>;
}

pub struct Locator {
    /// Current directory to begin search from in case dir is unspecified.
    cwd: PathBuf,

    /// Specific directory to look in (overrides `cwd`).
    dir: Option<PathBuf>,
}

impl Locator {
    pub fn new(cwd: PathBuf, dir: Option<PathBuf>) -> Self {
        Self { cwd, dir }
    }
}

impl Locate for Locator {
    fn locate(&self) -> Result<PathBuf, LocateError> {
        // Specified path
        if let Some(dir) = &self.dir {
            if !dir.join(PROJECT_MANIFEST).exists() {
                return Err(LocateError::NotFound(dir.to_owned()));
            }

            return Ok(dir.to_owned());
        }

        // Unspecified path
        let mut dir = self.cwd.to_owned();

        loop {
            if !dir.join(PROJECT_MANIFEST).exists() {
                if let Some(p) = dir.parent() {
                    dir = p.to_path_buf();
                    continue;
                }

                return Err(LocateError::NotFound(self.cwd.to_owned()));
            }

            return Ok(dir);
        }
    }
}

#[cfg(test)]
mod tests {

    // #[test]
    // fn default_networks() -> Result<(), Error> {
    //     assert_eq!(
    //         default_networks_def(),
    //         vec![
    //             Item::Manifest(NetworkManifest {
    //                 name: "local".to_string(),
    //                 configuration: Some(Configuration::Managed {
    //                     managed: Managed {
    //                         gateway: Gateway {
    //                             host: "localhost".to_string(),
    //                             port: Port::Fixed(8000),
    //                         },
    //                     }
    //                 }),
    //             }),
    //             Item::Manifest(NetworkManifest {
    //                 name: "mainnet".to_string(),
    //                 configuration: Some(Configuration::Connected {
    //                     connected: Connected {
    //                         url: "https://icp-api.io".to_string(),
    //                         root_key: None,
    //                     }
    //                 }),
    //             }),
    //         ]
    //     );

    //     Ok(())
    // }
}
