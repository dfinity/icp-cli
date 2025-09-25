use crate::{
    manifest::{
        IdentityKeyAlgorithm, IdentityList, IdentitySpec, LoadIdentityManifestError, PemFormat,
        WriteIdentityManifestError, load_identity_defaults, load_identity_list,
        write_identity_list,
    },
    paths::{ensure_key_pem_path, key_pem_path},
};
use camino::{Utf8Path, Utf8PathBuf};
use ic_agent::{
    Identity,
    identity::{AnonymousIdentity, Secp256k1Identity},
};
use icp_dirs::IcpCliDirs;
use icp_fs::fs;
use pem::Pem;
use pkcs8::{
    DecodePrivateKey, EncodePrivateKey, EncryptedPrivateKeyInfo, PrivateKeyInfo, SecretDocument,
    pkcs5::pbes2::Parameters,
};
use rand::RngCore;
use scrypt::Params;
use sec1::{der::Decode, pem::PemLabel};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use std::sync::Arc;
use zeroize::Zeroizing;

pub fn load_identity(
    dirs: &IcpCliDirs,
    list: &IdentityList,
    name: &str,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    // todo support p256, ed25519
    let identity = list
        .identities
        .get(name)
        .context(NoSuchIdentitySnafu { name })?;
    match identity {
        IdentitySpec::Pem {
            format,
            algorithm,
            principal: _,
        } => load_pem_identity(dirs, name, format, algorithm, password_func),
        IdentitySpec::Anonymous => Ok(Arc::new(AnonymousIdentity)),
    }
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityError {
    #[snafu(transparent)]
    ReadFileError { source: fs::ReadToStringError },

    #[snafu(display("failed to load PEM file `{path}`: failed to parse"))]
    ParsePemError {
        path: Utf8PathBuf,
        source: pem::PemError,
    },

    #[snafu(display("failed to load PEM file `{path}`: failed to decipher key"))]
    ParseKeyError {
        path: Utf8PathBuf,
        source: pkcs8::Error,
    },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },

    #[snafu(display("failed to read password: {message}"))]
    GetPasswordError { message: String },
}

fn load_pem_identity(
    dirs: &IcpCliDirs,
    name: &str,
    format: &PemFormat,
    algorithm: &IdentityKeyAlgorithm,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let pem_path = key_pem_path(dirs, name);
    let pem = fs::read_to_string(&pem_path)?;
    let doc = pem
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
    path: &Utf8Path,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    assert!(
        doc.tag() == pkcs8::EncryptedPrivateKeyInfo::PEM_LABEL,
        "internal error: wrong identity format found"
    );
    let password =
        password_func().map_err(|message| LoadIdentityError::GetPasswordError { message })?;
    match algorithm {
        IdentityKeyAlgorithm::Secp256k1 => {
            let key = k256::SecretKey::from_pkcs8_encrypted_der(doc.contents(), &password)
                .context(ParseKeySnafu { path })?;
            Ok(Arc::new(Secp256k1Identity::from_private_key(key)))
        }
    }
}

fn load_plaintext_identity(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    path: &Utf8Path,
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

pub fn load_identity_in_context(
    dirs: &IcpCliDirs,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
    let defaults = load_identity_defaults(dirs)?;
    let list = load_identity_list(dirs)?;
    let identity = load_identity(dirs, &list, &defaults.default, password_func)?;
    Ok(identity)
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityInContextError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityError },

    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },
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
    let principal = match &key {
        IdentityKey::Secp256k1(secret_key) => {
            Secp256k1Identity::from_private_key(secret_key.clone())
                .sender()
                .expect("infallible method")
        }
    };
    let spec = IdentitySpec::Pem {
        format: pem_format,
        algorithm,
        principal,
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
        CreateFormat::Pbes2 { password } => make_pkcs5_encrypted_pem(&doc, &password),
    };
    write_identity(dirs, name, &pem)?;
    identity_list.identities.insert(name.to_string(), spec);
    write_identity_list(dirs, &identity_list)?;
    Ok(())
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

fn write_identity(dirs: &IcpCliDirs, name: &str, pem: &str) -> Result<(), WriteIdentityError> {
    let pem_path = ensure_key_pem_path(dirs, name)?;
    fs::write(&pem_path, pem.as_bytes())?;
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityError {
    #[snafu(transparent)]
    WriteFileError { source: fs::WriteFileError },

    #[snafu(transparent)]
    CreateDirectoryError { source: fs::CreateDirAllError },
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
