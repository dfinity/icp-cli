use std::{collections::HashMap, io::ErrorKind, path::PathBuf, sync::Arc};

use ic_agent::{
    Identity,
    identity::{AnonymousIdentity, Secp256k1Identity},
};
use icp_dirs::IcpCliDirs;
use icp_fs::fs::{self, CreateDirAllError, WriteFileError};
use itertools::Itertools;
use pem::Pem;
use pkcs8::{
    DecodePrivateKey, EncodePrivateKey, EncryptedPrivateKeyInfo, PrivateKeyInfo,
    der::pem::PemLabel, pkcs5::pbes2::Parameters,
};
use rand::RngCore;
use scrypt::Params;
use sec1::der::Decode;
use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use zeroize::Zeroizing;

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
    // Keyring,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityKeyAlgorithm {
    Secp256k1,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PemFormat {
    Plaintext,
    Pbes2,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct IdentityList {
    pub v: u32,
    pub identities: HashMap<String, IdentitySpec>,
}

impl IdentityList {
    pub fn to_valid_identity_names(&self) -> Vec<String> {
        let mut names = self.identities.keys().cloned().collect_vec();
        names.extend(special_identities());
        names
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct IdentityDefaults {
    pub v: u32,
    pub default: String,
}

pub fn load_identity_list(dirs: &IcpCliDirs) -> Result<IdentityList, LoadIdentityError> {
    let id_list_file = identity_list_path(dirs);
    let list = match fs::read_to_string(&id_list_file) {
        Ok(id_list) => serde_json::from_str(&id_list).context(ParseJsonSnafu {
            path: &id_list_file,
        })?,
        Err(e) if e.source.kind() == ErrorKind::NotFound => {
            let empty_list = IdentityList {
                v: 1,
                identities: HashMap::new(),
            };
            write_identity_list(dirs, &empty_list).context(WriteDefaultsSnafu)?;
            empty_list
        }
        Err(e) => {
            return Err(e.into());
        }
    };
    Ok(list)
}

pub fn load_identity(
    dirs: &IcpCliDirs,
    list: &IdentityList,
    name: &str,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    // todo support p256, ed25519
    if name == "anonymous" {
        return Ok(Arc::new(AnonymousIdentity));
    }
    let identity = list
        .identities
        .get(name)
        .context(NoSuchIdentitySnafu { name })?;
    match identity {
        IdentitySpec::Pem { format, algorithm } => {
            let pem_path = key_pem_path(dirs, name);
            let pem = fs::read_to_string(&pem_path)?;
            let doc = pem
                .parse::<Pem>()
                .context(ParsePemSnafu { path: &pem_path })?;
            match format {
                PemFormat::Pbes2 => {
                    assert!(
                        doc.tag() == pkcs8::EncryptedPrivateKeyInfo::PEM_LABEL,
                        "internal error: wrong identity format found"
                    );
                    let password = password_func()
                        .map_err(|message| LoadIdentityError::GetPasswordError { message })?;
                    match algorithm {
                        IdentityKeyAlgorithm::Secp256k1 => {
                            let key = k256::SecretKey::from_pkcs8_encrypted_der(
                                doc.contents(),
                                &password,
                            )
                            .context(ParseKeySnafu { path: &pem_path })?;
                            Ok(Arc::new(Secp256k1Identity::from_private_key(key)))
                        }
                    }
                }
                PemFormat::Plaintext => {
                    assert!(
                        doc.tag() == PrivateKeyInfo::PEM_LABEL,
                        "internal error: wrong identity format found"
                    );
                    match algorithm {
                        IdentityKeyAlgorithm::Secp256k1 => {
                            let key = k256::SecretKey::from_pkcs8_der(doc.contents())
                                .context(ParseKeySnafu { path: &pem_path })?;
                            Ok(Arc::new(Secp256k1Identity::from_private_key(key)))
                        }
                    }
                }
            }
        }
    }
}

pub fn load_identity_defaults(dirs: &IcpCliDirs) -> Result<IdentityDefaults, LoadIdentityError> {
    let id_defaults_path = identity_defaults_path(dirs);
    match fs::read_to_string(&id_defaults_path) {
        Ok(id_defaults) => Ok(serde_json::from_str(&id_defaults).context(ParseJsonSnafu {
            path: &id_defaults_path,
        })?),
        Err(e) if e.source.kind() == ErrorKind::NotFound => {
            let empty_defaults = IdentityDefaults {
                v: 1,
                default: "anonymous".to_string(),
            };
            write_identity_defaults(dirs, &empty_defaults).context(WriteDefaultsSnafu)?;
            Ok(empty_defaults)
        }
        Err(e) => Err(e.into()),
    }
}

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

#[derive(Debug, Clone)]
pub enum IdentityKey {
    Secp256k1(k256::SecretKey),
}

#[derive(Debug, Clone)]
pub enum CreateFormat {
    Plaintext,
    Pbes2 { password: Zeroizing<String> },
    // Keyring,
}

pub fn create_identity(
    dirs: &IcpCliDirs,
    name: &str,
    key: IdentityKey,
    format: CreateFormat,
) -> Result<(), CreateIdentityError> {
    let algorithm = match key {
        IdentityKey::Secp256k1(_) => IdentityKeyAlgorithm::Secp256k1,
    };
    let pem_format = match format {
        CreateFormat::Plaintext => PemFormat::Plaintext,
        CreateFormat::Pbes2 { .. } => PemFormat::Pbes2,
    };
    let spec = IdentitySpec::Pem {
        format: pem_format,
        algorithm,
    };
    let mut identity_list = load_identity_list(dirs)?;
    ensure!(
        !identity_list.identities.contains_key(name),
        IdentityAlreadyExistsSnafu { name }
    );
    let doc = match key {
        IdentityKey::Secp256k1(key) => key.to_pkcs8_der().expect("infallible PKI encoding"),
    };
    let pem = match format {
        CreateFormat::Plaintext => doc
            .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
            .expect("infallible PKI encoding"),
        CreateFormat::Pbes2 { password } => {
            let pki = PrivateKeyInfo::from_der(doc.as_bytes()).expect("infallible PKI roundtrip");
            let mut salt = [0; 16];
            let mut iv = [0; 16];
            let mut rng = rand::rng();
            rng.fill_bytes(&mut salt);
            rng.fill_bytes(&mut iv);
            pki.encrypt_with_params(
                Parameters::scrypt_aes256cbc(
                    Params::new(17, 8, 1, 32).expect("valid scrypt params"),
                    &salt,
                    &iv,
                )
                .expect("valid pbes2 params"),
                password,
            )
            .expect("infallible PKI encryption")
            .to_pem(EncryptedPrivateKeyInfo::PEM_LABEL, Default::default())
            .expect("infallible EPKI encoding")
        }
    };
    let pem_path = key_pem_path(dirs, name);
    let parent = pem_path.parent().unwrap();
    fs::create_dir_all(parent).map_err(WriteIdentityError::from)?;
    fs::write(&pem_path, pem.as_bytes()).map_err(WriteIdentityError::from)?;
    identity_list.identities.insert(name.to_string(), spec);
    write_identity_list(dirs, &identity_list)?;
    Ok(())
}

pub fn load_identity_in_context(
    dirs: &IcpCliDirs,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let defaults = load_identity_defaults(dirs)?;
    let list = load_identity_list(dirs)?;
    load_identity(dirs, &list, &defaults.default, password_func)
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityError {
    #[snafu(display("failed to write configuration defaults"))]
    WriteDefaultsError { source: WriteIdentityError },

    #[snafu(transparent)]
    ReadFileError { source: fs::ReadFileError },

    #[snafu(display("failed to parse json at `{}`", path.display()))]
    ParseJsonError {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[snafu(display("failed to load PEM file `{}`: failed to parse", path.display()))]
    ParsePemError {
        path: PathBuf,
        source: pem::PemError,
    },

    #[snafu(display("failed to load PEM file `{}`: failed to decipher key", path.display()))]
    ParseKeyError { path: PathBuf, source: pkcs8::Error },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },

    #[snafu(display("failed to read password: {message}"))]
    GetPasswordError { message: String },
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityError {
    #[snafu(transparent)]
    WriteFileError { source: WriteFileError },

    #[snafu(transparent)]
    CreateDirectoryError { source: CreateDirAllError },
}

#[derive(Debug, Snafu)]
pub enum CreateIdentityError {
    #[snafu(transparent)]
    Load { source: LoadIdentityError },

    #[snafu(transparent)]
    Write { source: WriteIdentityError },

    #[snafu(display("identity `{name}` already exists"))]
    IdentityAlreadyExists { name: String },
}

pub fn identity_defaults_path(dirs: &IcpCliDirs) -> PathBuf {
    dirs.identity_dir().join("identity_defaults.json")
}

pub fn identity_list_path(dirs: &IcpCliDirs) -> PathBuf {
    dirs.identity_dir().join("identity_list.json")
}

pub fn key_pem_path(dirs: &IcpCliDirs, name: &str) -> PathBuf {
    dirs.identity_dir().join(format!("keys/{name}.pem"))
}

pub fn special_identities() -> Vec<String> {
    vec!["anonymous".to_string()]
}
