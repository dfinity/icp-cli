use crate::{LoadIdentityError, WriteIdentityError, s_load::*};
use camino::Utf8PathBuf;
use icp_dirs::IcpCliDirs;
use icp_fs::fs;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu, ensure};
use std::{collections::HashMap, io::ErrorKind};

pub fn write_identity_defaults(
    dirs: &IcpCliDirs,
    defaults: &IdentityDefaults,
) -> Result<(), WriteIdentityError> {
    let defaults_path = identity_defaults_path(dirs);
    let parent = defaults_path.parent().unwrap();
    fs::create_dir_all(parent)?;
    let json = serde_json::to_string(defaults).unwrap();
    fs::write(&defaults_path, json.as_bytes())?;
    Ok(())
}

pub fn write_identity_list(
    dirs: &IcpCliDirs,
    list: &IdentityList,
) -> Result<(), WriteIdentityError> {
    let defaults_path = identity_list_path(dirs);
    let parent = defaults_path.parent().unwrap();
    fs::create_dir_all(parent)?;
    let json = serde_json::to_string(list).unwrap();
    fs::write(&defaults_path, json.as_bytes())?;
    Ok(())
}

pub fn load_identity_list(dirs: &IcpCliDirs) -> Result<IdentityList, LoadIdentityError> {
    let id_list_file = identity_list_path(dirs);
    let list = match fs::read_to_string(&id_list_file) {
        Ok(id_list) => serde_json::from_str(&id_list).context(ParseJsonSnafu {
            path: &id_list_file,
        })?,
        Err(e) if e.source.kind() == ErrorKind::NotFound => IdentityList::default(),
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

pub fn load_identity_defaults(dirs: &IcpCliDirs) -> Result<IdentityDefaults, LoadIdentityError> {
    let id_defaults_path = identity_defaults_path(dirs);
    let defaults = match fs::read_to_string(&id_defaults_path) {
        Ok(id_defaults) => serde_json::from_str(&id_defaults).context(ParseJsonSnafu {
            path: &id_defaults_path,
        })?,
        Err(e) if e.source.kind() == ErrorKind::NotFound => IdentityDefaults::default(),
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
    Load { source: LoadIdentityError },
    #[snafu(transparent)]
    Write { source: WriteIdentityError },
    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },
}

pub fn identity_defaults_path(dirs: &IcpCliDirs) -> Utf8PathBuf {
    dirs.identity_dir().join("identity_defaults.json")
}

pub fn identity_list_path(dirs: &IcpCliDirs) -> Utf8PathBuf {
    dirs.identity_dir().join("identity_list.json")
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
    },
    Anonymous,
    // Keyring,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PemFormat {
    Plaintext,
    Pbes2,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityKeyAlgorithm {
    Secp256k1,
}
