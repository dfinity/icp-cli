use std::{collections::HashMap, io::ErrorKind};

use ic_agent::export::Principal;
use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};
use strum::{Display, EnumString};

use crate::{
    fs::{
        json,
        lock::{LRead, LWrite},
    },
    identity::IdentityPaths,
    prelude::*,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct IdentityDefaults {
    pub v: u32,
    pub default: String,
}

impl IdentityDefaults {
    pub fn write_to(&self, dirs: LWrite<&IdentityPaths>) -> Result<(), WriteIdentityManifestError> {
        json::save(&dirs.identity_defaults_path(), self)?;
        Ok(())
    }

    pub fn load_from(dirs: LRead<&IdentityPaths>) -> Result<Self, LoadIdentityManifestError> {
        let id_defaults_path = dirs.identity_defaults_path();

        let defaults = json::load(&id_defaults_path).or_else(|err| match err {
            // Default fallback
            json::Error::Io { source } if source.kind() == ErrorKind::NotFound => {
                Ok(Self::default())
            }

            // Other
            _ => Err(err),
        })?;

        ensure!(
            defaults.v == 1,
            BadVersionSnafu {
                path: &id_defaults_path
            }
        );

        Ok(defaults)
    }
}

impl Default for IdentityDefaults {
    fn default() -> Self {
        Self {
            v: 1,
            default: "anonymous".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct IdentityList {
    pub v: u32,
    pub identities: HashMap<String, IdentitySpec>,
}

impl Default for IdentityList {
    fn default() -> Self {
        Self {
            v: 1,
            identities: HashMap::from([("anonymous".to_string(), IdentitySpec::Anonymous)]),
        }
    }
}

impl IdentityList {
    pub fn write_to(&self, dirs: LWrite<&IdentityPaths>) -> Result<(), WriteIdentityManifestError> {
        json::save(&dirs.identity_list_path(), self)?;
        Ok(())
    }
    pub fn load_from(dirs: LRead<&IdentityPaths>) -> Result<Self, LoadIdentityManifestError> {
        let id_list_file = dirs.identity_list_path();

        let list = json::load(&id_list_file).or_else(|err| match err {
            // Default fallback
            json::Error::Io { source } if source.kind() == ErrorKind::NotFound => {
                Ok(Self::default())
            }

            // Other
            _ => Err(err),
        })?;

        ensure!(
            list.v == 1,
            BadVersionSnafu {
                path: &id_list_file
            }
        );

        Ok(list)
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
pub enum IdentitySpec {
    Pem {
        format: PemFormat,
        algorithm: IdentityKeyAlgorithm,
        principal: Principal,
    },
    Anonymous,
    // Keyring,
}

impl IdentitySpec {
    pub fn principal(&self) -> Principal {
        match self {
            IdentitySpec::Pem { principal, .. } => *principal,
            IdentitySpec::Anonymous => Principal::anonymous(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PemFormat {
    Plaintext,
    Pbes2,
}

#[derive(Deserialize, Serialize, Clone, Debug, EnumString, Display)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum IdentityKeyAlgorithm {
    #[serde(rename = "secp256k1", alias = "k256")]
    #[strum(serialize = "secp256k1", serialize = "k256")]
    #[cfg_attr(feature = "clap", value(alias = "k256"))]
    Secp256k1,
    #[serde(rename = "prime256v1", alias = "p256", alias = "secp256r1")]
    #[strum(serialize = "prime256v1", serialize = "p256", serialize = "secp256r1")]
    #[cfg_attr(feature = "clap", value(alias = "p256", alias = "secp256r1"))]
    Prime256v1,
    #[serde(rename = "ed25519")]
    #[strum(serialize = "ed25519")]
    Ed25519,
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityManifestError {
    #[snafu(transparent)]
    WriteJsonError { source: json::Error },

    #[snafu(transparent)]
    CreateDirectoryError { source: crate::fs::IoError },

    #[snafu(transparent)]
    DirectoryLockError { source: crate::fs::lock::LockError },
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityManifestError {
    #[snafu(transparent)]
    LoadJsonError { source: json::Error },

    #[snafu(display("file `{path}` was modified by an incompatible new version of icp-cli"))]
    BadVersion { path: PathBuf },

    #[snafu(transparent)]
    DirectoryLockError { source: crate::fs::lock::LockError },
}

#[derive(Debug, Snafu)]
pub enum ChangeDefaultsError {
    #[snafu(transparent)]
    Load { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    Write { source: WriteIdentityManifestError },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },
}

pub fn change_default_identity(
    dirs: LWrite<&IdentityPaths>,
    list: &IdentityList,
    name: &str,
) -> Result<(), ChangeDefaultsError> {
    ensure!(
        list.identities.contains_key(name),
        NoSuchIdentitySnafu { name }
    );

    let mut defaults = IdentityDefaults::load_from(dirs.read())?;
    defaults.default = name.to_string();
    defaults.write_to(dirs)?;

    Ok(())
}
