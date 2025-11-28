use std::sync::Arc;

use ic_agent::{
    Identity,
    identity::{AnonymousIdentity, Secp256k1Identity},
};
use pem::Pem;
use pkcs8::{
    DecodePrivateKey, EncodePrivateKey, EncryptedPrivateKeyInfo, PrivateKeyInfo, SecretDocument,
    pkcs5::pbes2::Parameters,
};
use rand::RngCore;
use scrypt::Params;
use sec1::{der::Decode, pem::PemLabel};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use zeroize::Zeroizing;

use crate::{
    fs::{
        self,
        lock::{LRead, LWrite},
    },
    identity::{
        IdentityPaths,
        manifest::{
            IdentityDefaults, IdentityKeyAlgorithm, IdentityList, IdentitySpec,
            LoadIdentityManifestError, PemFormat, WriteIdentityManifestError,
        },
    },
    prelude::*,
};

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

#[derive(Debug, Snafu)]
pub enum LoadIdentityError {
    #[snafu(transparent)]
    ReadFileError { source: crate::fs::Error },

    #[snafu(display("failed to load PEM file `{path}`: failed to parse"))]
    ParsePemError {
        path: PathBuf,
        #[snafu(source(from(pem::PemError, Box::new)))]
        source: Box<pem::PemError>,
    },

    #[snafu(display("failed to load PEM file `{path}`: failed to decipher key"))]
    ParseKeyError {
        path: PathBuf,
        #[snafu(source(from(pkcs8::Error, Box::new)))]
        source: Box<pkcs8::Error>,
    },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },

    #[snafu(display("failed to read password: {message}"))]
    GetPasswordError { message: String },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },
}

// TODO(adam.spofford): Support p256, ed25519
pub fn load_identity(
    dirs: LRead<&IdentityPaths>,
    list: &IdentityList,
    name: &str,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let identity = list
        .identities
        .get(name)
        .context(NoSuchIdentitySnafu { name })?;

    match identity {
        IdentitySpec::Pem {
            format, algorithm, ..
        } => load_pem_identity(dirs, name, format, algorithm, password_func),

        IdentitySpec::Anonymous => Ok(Arc::new(AnonymousIdentity)),
    }
}

fn load_pem_identity(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    format: &PemFormat,
    algorithm: &IdentityKeyAlgorithm,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let pem_path = dirs.key_pem_path(name);

    let doc = fs::read_to_string(&pem_path)?
        .parse::<Pem>()
        .context(ParsePemSnafu { path: &pem_path })?;

    match format {
        PemFormat::Pbes2 => load_pbes2_identity(&doc, algorithm, password_func, &pem_path),

        PemFormat::Plaintext => load_plaintext_identity(&doc, algorithm, &pem_path),
    }
}

fn load_pbes2_identity(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    password_func: impl FnOnce() -> Result<String, String>,
    path: &Path,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    assert!(
        doc.tag() == pkcs8::EncryptedPrivateKeyInfo::PEM_LABEL,
        "internal error: wrong identity format found"
    );

    let pw = password_func().map_err(|message| LoadIdentityError::GetPasswordError { message })?;

    match algorithm {
        IdentityKeyAlgorithm::Secp256k1 => {
            let key = k256::SecretKey::from_pkcs8_encrypted_der(doc.contents(), &pw)
                .context(ParseKeySnafu { path })?;

            Ok(Arc::new(Secp256k1Identity::from_private_key(key)))
        }
    }
}

fn load_plaintext_identity(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    path: &Path,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    assert!(
        doc.tag() == PrivateKeyInfo::PEM_LABEL,
        "internal error: wrong identity format found"
    );

    match algorithm {
        IdentityKeyAlgorithm::Secp256k1 => {
            let key =
                k256::SecretKey::from_pkcs8_der(doc.contents()).context(ParseKeySnafu { path })?;

            Ok(Arc::new(Secp256k1Identity::from_private_key(key)))
        }
    }
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityInContextError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityError },

    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },
}

pub async fn load_identity_in_context(
    dirs: LRead<&IdentityPaths>,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
    let identity = load_identity(
        dirs,
        &IdentityList::load_from(dirs)?,
        &(IdentityDefaults::load_from(dirs)?).default,
        password_func,
    )?;

    Ok(identity)
}

#[derive(Debug, Snafu)]
pub enum CreateIdentityError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityError },

    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    WriteIdentityManifest { source: WriteIdentityManifestError },

    #[snafu(transparent)]
    WriteIdentity { source: WriteIdentityError },

    #[snafu(display("identity `{name}` already exists"))]
    IdentityAlreadyExists { name: String },
}

pub fn create_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    key: IdentityKey,
    format: CreateFormat,
) -> Result<(), CreateIdentityError> {
    let spec = IdentitySpec::Pem {
        format: match format {
            CreateFormat::Plaintext => PemFormat::Plaintext,
            CreateFormat::Pbes2 { .. } => PemFormat::Pbes2,
        },

        algorithm: match key {
            IdentityKey::Secp256k1(_) => IdentityKeyAlgorithm::Secp256k1,
        },

        principal: match &key {
            IdentityKey::Secp256k1(secret_key) => {
                Secp256k1Identity::from_private_key(secret_key.clone())
                    .sender()
                    .expect("infallible method")
            }
        },
    };

    let mut identity_list = IdentityList::load_from(dirs.read())?;
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

        CreateFormat::Pbes2 { password } => make_pkcs5_encrypted_pem(&doc, &password),
    };

    write_identity(dirs, name, &pem)?;
    identity_list.identities.insert(name.to_string(), spec);
    identity_list.write_to(dirs)?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityError {
    #[snafu(display("failed to write file"))]
    WriteFileError { source: crate::fs::Error },

    #[snafu(display("failed to create directory"))]
    CreateDirectoryError { source: crate::fs::Error },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },
}

fn write_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    pem: &str,
) -> Result<(), WriteIdentityError> {
    let pem_path = dirs.ensure_key_pem_path(name).context(WriteFileSnafu)?;
    fs::write_string(&pem_path, pem).context(WriteFileSnafu)?;

    Ok(())
}

fn make_pkcs5_encrypted_pem(doc: &SecretDocument, password: &str) -> Zeroizing<String> {
    let pki = PrivateKeyInfo::from_der(doc.as_bytes()).expect("infallible PKI roundtrip");
    let mut salt = [0; 16];
    let mut iv = [0; 16];

    let mut rng = rand::rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut iv);

    let encrypted_doc = pki
        .encrypt_with_params(
            Parameters::scrypt_aes256cbc(
                Params::new(17, 8, 1, 32).expect("valid scrypt params"),
                &salt,
                &iv,
            )
            .expect("valid pbes2 params"),
            password,
        )
        .expect("infallible PKI encryption");

    encrypted_doc
        .to_pem(EncryptedPrivateKeyInfo::PEM_LABEL, Default::default())
        .expect("infallible EPKI encoding")
}
