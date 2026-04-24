use std::{collections::HashMap, io::ErrorKind};

use ic_agent::export::Principal;
use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};
use strum::{Display, EnumString};
use url::Url;

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
    Keyring {
        principal: Principal,
        algorithm: IdentityKeyAlgorithm,
    },
    Hsm {
        principal: Principal,
        module: PathBuf,
        slot: usize,
        key_id: String,
    },
    InternetIdentity {
        algorithm: IdentityKeyAlgorithm,
        /// The principal at the root of the delegation chain
        /// (`Principal::self_authenticating(from_key)`), not the session key.
        principal: Principal,
        storage: DelegationKeyStorage,
        /// The host used for II login, stored so `icp identity login` can
        /// re-authenticate without requiring `--host` again.
        host: Url,
    },
    /// Session key created via `icp identity delegation request`; no delegation
    /// chain yet. Cannot be used as an identity until `icp identity delegation use`
    /// completes it.
    PendingDelegation {
        algorithm: IdentityKeyAlgorithm,
        storage: DelegationKeyStorage,
    },
    /// Fully delegated identity created via `icp identity delegation use`.
    /// Behaves identically to `InternetIdentity` when loaded.
    Delegation {
        algorithm: IdentityKeyAlgorithm,
        /// `Principal::self_authenticating(chain.public_key)` — root key principal.
        principal: Principal,
        storage: DelegationKeyStorage,
    },
}

impl IdentitySpec {
    /// Returns the principal associated with this identity, or `None` if the
    /// identity has no delegation yet (`PendingDelegation`).
    pub fn principal(&self) -> Option<Principal> {
        match self {
            IdentitySpec::Pem { principal, .. } => Some(*principal),
            IdentitySpec::Anonymous => Some(Principal::anonymous()),
            IdentitySpec::Keyring { principal, .. } => Some(*principal),
            IdentitySpec::Hsm { principal, .. } => Some(*principal),
            IdentitySpec::InternetIdentity { principal, .. } => Some(*principal),
            IdentitySpec::PendingDelegation { .. } => None,
            IdentitySpec::Delegation { principal, .. } => Some(*principal),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PemFormat {
    Plaintext,
    Pbes2,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum DelegationKeyStorage {
    Keyring,
    Pem { format: PemFormat },
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
