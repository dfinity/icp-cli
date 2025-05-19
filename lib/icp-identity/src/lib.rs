use std::{
    collections::HashMap,
    fs,
    io::{self, ErrorKind},
    path::PathBuf,
    sync::Arc,
};

use directories::ProjectDirs;
use ic_agent::{
    Identity,
    identity::{AnonymousIdentity, Secp256k1Identity},
};
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
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    tag = "kind"
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
pub struct IdentityList {
    pub v: u32,
    pub identities: HashMap<String, IdentitySpec>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IdentityDefaults {
    pub v: u32,
    pub default: String,
}

pub fn load_identity_list() -> Result<IdentityList, LoadIdentityError> {
    let id_list_file = identity_list_path();
    let list = match fs::read_to_string(&id_list_file) {
        Ok(id_list) => serde_json::from_str(&id_list).context(ParseJsonSnafu {
            path: &id_list_file,
        })?,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let empty_list = IdentityList {
                v: 1,
                identities: HashMap::new(),
            };
            write_identity_list(&empty_list).context(WriteDefaultsSnafu)?;
            empty_list
        }
        Err(e) => {
            return Err(e).context(ReadFileSnafu {
                path: &id_list_file,
            });
        }
    };
    Ok(list)
}

pub fn load_identity(
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
            let pem_path = key_pem_path(name);
            let pem = fs::read_to_string(&pem_path).context(ReadFileSnafu { path: &pem_path })?;
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

pub fn load_identity_defaults() -> Result<IdentityDefaults, LoadIdentityError> {
    let id_defaults_path = identity_defaults_path();
    match fs::read_to_string(&id_defaults_path) {
        Ok(id_defaults) => Ok(serde_json::from_str(&id_defaults).context(ParseJsonSnafu {
            path: &id_defaults_path,
        })?),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let empty_defaults = IdentityDefaults {
                v: 1,
                default: "anonymous".to_string(),
            };
            write_identity_defaults(&empty_defaults).context(WriteDefaultsSnafu)?;
            Ok(empty_defaults)
        }
        Err(e) => Err(e).context(ReadFileSnafu {
            path: id_defaults_path,
        }),
    }
}

pub fn write_identity_defaults(defaults: &IdentityDefaults) -> Result<(), WriteIdentityError> {
    let defaults_path = identity_defaults_path();
    let parent = defaults_path.parent().unwrap();
    fs::create_dir_all(parent).context(CreateDirectorySnafu { path: parent })?;
    let json = serde_json::to_string(defaults).unwrap();
    fs::write(&defaults_path, json.as_bytes()).context(WriteFileSnafu {
        path: &defaults_path,
    })?;
    Ok(())
}

pub fn write_identity_list(list: &IdentityList) -> Result<(), WriteIdentityError> {
    let defaults_path = identity_defaults_path();
    let parent = defaults_path.parent().unwrap();
    fs::create_dir_all(parent).context(CreateDirectorySnafu { path: parent })?;
    let json = serde_json::to_string(list).unwrap();
    fs::write(&defaults_path, json.as_bytes()).context(WriteFileSnafu {
        path: &defaults_path,
    })?;
    Ok(())
}

pub enum IdentityKey {
    Secp256k1(k256::SecretKey),
}

pub enum CreateFormat {
    Plaintext,
    Pbes2 { password: Zeroizing<String> },
    // Keyring,
}

pub fn create_identity(
    name: &str,
    key: IdentityKey,
    format: CreateFormat,
) -> Result<(), CreateIdentityError> {
    let algorithm = match key {
        IdentityKey::Secp256k1(_) => IdentityKeyAlgorithm::Secp256k1,
    };
    let spec = match format {
        CreateFormat::Plaintext => IdentitySpec::Pem {
            format: PemFormat::Plaintext,
            algorithm,
        },
        CreateFormat::Pbes2 { .. } => IdentitySpec::Pem {
            format: PemFormat::Pbes2,
            algorithm,
        },
    };
    let mut identity_list = load_identity_list()?;
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
    let pem_path = key_pem_path(name);
    let parent = pem_path.parent().unwrap();
    fs::create_dir_all(parent).context(CreateDirectorySnafu { path: parent })?;
    fs::write(&pem_path, pem.as_bytes()).context(WriteFileSnafu { path: parent })?;
    identity_list.identities.insert(name.to_string(), spec);
    write_identity_list(&identity_list)?;
    Ok(())
}

pub fn load_identity_in_context(
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let defaults = load_identity_defaults()?;
    let list = load_identity_list()?;
    load_identity(&list, &defaults.default, password_func)
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityError {
    WriteDefaultsError {
        source: WriteIdentityError,
    },
    ReadFileError {
        path: PathBuf,
        source: io::Error,
    },
    ParseJsonError {
        path: PathBuf,
        source: serde_json::Error,
    },
    ParsePemError {
        path: PathBuf,
        source: pem::PemError,
    },
    ParseKeyError {
        path: PathBuf,
        source: pkcs8::Error,
    },
    NoSuchIdentity {
        name: String,
    },
    GetPasswordError {
        message: String,
    },
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityError {
    WriteFileError { path: PathBuf, source: io::Error },
    CreateDirectoryError { path: PathBuf, source: io::Error },
}

#[derive(Debug, Snafu)]
pub enum CreateIdentityError {
    #[snafu(transparent)]
    Load {
        source: LoadIdentityError,
    },
    #[snafu(transparent)]
    Write {
        source: WriteIdentityError,
    },
    IdentityAlreadyExists {
        name: String,
    },
}

pub fn identity_defaults_path() -> PathBuf {
    data_dir().join("identity_defaults.json")
}

pub fn identity_list_path() -> PathBuf {
    data_dir().join("identity_list.json")
}

pub fn key_pem_path(name: &str) -> PathBuf {
    data_dir().join(format!("keys/{name}.pem"))
}

fn data_dir() -> PathBuf {
    ProjectDirs::from("org.dfinity", "DFINITY Stiftung", "icp-cli")
        .unwrap()
        .data_dir()
        .to_path_buf()
}
