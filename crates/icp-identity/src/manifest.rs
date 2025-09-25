use crate::paths::{
    ensure_identity_defaults_path, ensure_identity_list_path, identity_defaults_path,
    identity_list_path,
};
use camino::Utf8PathBuf;
use ic_agent::export::Principal;
use icp_dirs::IcpCliDirs;
use icp_fs::{
    fs,
    json::{self, LoadJsonFileError},
};
use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};
use std::{collections::HashMap, io::ErrorKind};
use strum::{Display, EnumString};

pub fn write_identity_defaults(
    dirs: &IcpCliDirs,
    defaults: &IdentityDefaults,
) -> Result<(), WriteIdentityManifestError> {
    let defaults_path = ensure_identity_defaults_path(dirs)?;
    json::save_json_file(&defaults_path, defaults)?;
    Ok(())
}

pub fn write_identity_list(
    dirs: &IcpCliDirs,
    list: &IdentityList,
) -> Result<(), WriteIdentityManifestError> {
    let defaults_path = ensure_identity_list_path(dirs)?;
    json::save_json_file(&defaults_path, list)?;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityManifestError {
    #[snafu(transparent)]
    WriteJsonError { source: json::SaveJsonFileError },

    #[snafu(transparent)]
    CreateDirectoryError { source: fs::CreateDirAllError },
}

pub fn load_identity_list(dirs: &IcpCliDirs) -> Result<IdentityList, LoadIdentityManifestError> {
    let id_list_file = identity_list_path(dirs);
    let list = match json::load_json_file(&id_list_file) {
        Ok(id_list) => id_list,
        Err(LoadJsonFileError::Read { source, .. })
            if source.source.kind() == ErrorKind::NotFound =>
        {
            IdentityList::default()
        }
        Err(e) => {
            return Err(e.into());
        }
    };
    ensure!(
        list.v == 1,
        BadVersionSnafu {
            path: &id_list_file
        }
    );
    Ok(list)
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityManifestError {
    #[snafu(transparent)]
    LoadJsonError { source: json::LoadJsonFileError },

    #[snafu(display("file `{path}` was modified by an incompatible new version of icp-cli"))]
    BadVersion { path: Utf8PathBuf },
}

pub fn load_identity_defaults(
    dirs: &IcpCliDirs,
) -> Result<IdentityDefaults, LoadIdentityManifestError> {
    let id_defaults_path = identity_defaults_path(dirs);
    let defaults = match json::load_json_file(&id_defaults_path) {
        Ok(id_defaults) => id_defaults,
        Err(LoadJsonFileError::Read { source, .. })
            if source.source.kind() == ErrorKind::NotFound =>
        {
            IdentityDefaults::default()
        }
        Err(e) => return Err(e.into()),
    };
    ensure!(
        defaults.v == 1,
        BadVersionSnafu {
            path: &id_defaults_path
        }
    );
    Ok(defaults)
}

pub fn change_default_identity(
    dirs: &IcpCliDirs,
    list: &IdentityList,
    name: &str,
) -> Result<(), ChangeDefaultsError> {
    ensure!(
        list.identities.contains_key(name),
        NoSuchIdentitySnafu { name }
    );
    let mut defaults = load_identity_defaults(dirs)?;
    defaults.default = name.to_string();
    write_identity_defaults(dirs, &defaults)?;
    Ok(())
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
