use std::{collections::HashMap, io::ErrorKind};

use ic_agent::export::Principal;
use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};
use strum::{Display, EnumString};

use crate::{
    fs::json,
    identity::{
        ensure_identity_defaults_path, ensure_identity_list_path, identity_defaults_path,
        identity_list_path,
    },
    prelude::*,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct IdentityDefaults {
    pub v: u32,
    pub default: String,
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
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityManifestError {
    #[snafu(transparent)]
    WriteJsonError { source: json::Error },

    #[snafu(transparent)]
    CreateDirectoryError { source: crate::fs::Error },
}

pub fn write_identity_defaults(
    dir: &Path,
    defaults: &IdentityDefaults,
) -> Result<(), WriteIdentityManifestError> {
    json::save(
        &ensure_identity_defaults_path(dir)?, // path
        defaults,                             // value
    )?;

    Ok(())
}

pub fn write_identity_list(
    dir: &Path,
    list: &IdentityList,
) -> Result<(), WriteIdentityManifestError> {
    json::save(
        &ensure_identity_list_path(dir)?, // path
        list,                             // value
    )?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityManifestError {
    #[snafu(transparent)]
    LoadJsonError { source: json::Error },

    #[snafu(display("file `{path}` was modified by an incompatible new version of icp-cli"))]
    BadVersion { path: PathBuf },
}

pub fn load_identity_list(dir: &Path) -> Result<IdentityList, LoadIdentityManifestError> {
    let id_list_file = identity_list_path(dir);

    let list = json::load(&id_list_file).or_else(|err| match err {
        // Default fallback
        json::Error::Io(err) if err.kind() == ErrorKind::NotFound => Ok(IdentityList::default()),

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

pub fn load_identity_defaults(dir: &Path) -> Result<IdentityDefaults, LoadIdentityManifestError> {
    let id_defaults_path = identity_defaults_path(dir);

    let defaults = json::load(&id_defaults_path).or_else(|err| match err {
        // Default fallback
        json::Error::Io(err) if err.kind() == ErrorKind::NotFound => {
            Ok(IdentityDefaults::default())
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
    dir: &Path,
    list: &IdentityList,
    name: &str,
) -> Result<(), ChangeDefaultsError> {
    ensure!(
        list.identities.contains_key(name),
        NoSuchIdentitySnafu { name }
    );

    let mut defaults = load_identity_defaults(dir)?;
    defaults.default = name.to_string();
    write_identity_defaults(dir, &defaults)?;

    Ok(())
}
