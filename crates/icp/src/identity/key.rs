use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use ic_agent::{
    Identity,
    identity::{AnonymousIdentity, BasicIdentity, Prime256v1Identity, Secp256k1Identity},
};
use ic_ed25519::PrivateKeyFormat;
use ic_identity_hsm::HardwareIdentity;
use keyring::Entry;
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
    Prime256v1(p256::SecretKey),
    Ed25519(ic_ed25519::PrivateKey),
}

#[derive(Debug, Clone)]
pub enum CreateFormat {
    Plaintext,
    Pbes2 { password: Zeroizing<String> },
    Keyring,
}

#[derive(Debug, Snafu)]
pub enum LoadIdentityError {
    #[snafu(transparent)]
    ReadFileError { source: crate::fs::IoError },

    #[snafu(display("failed to load PEM from `{origin}`: failed to parse"))]
    ParsePemError {
        origin: PemOrigin,
        #[snafu(source(from(pem::PemError, Box::new)))]
        source: Box<pem::PemError>,
    },

    #[snafu(display("failed to load PEM from `{origin}`: failed to decipher key"))]
    ParsePkcs8Error {
        origin: PemOrigin,
        #[snafu(source(from(pkcs8::Error, Box::new)))]
        source: Box<pkcs8::Error>,
    },
    #[snafu(display("failed to load PEM from `{origin}`: failed to decipher key"))]
    ParseDerError {
        origin: PemOrigin,
        source: pkcs8::der::Error,
    },
    #[snafu(display("failed to load PEM from `{origin}`: failed to decipher key"))]
    ParseEd25519KeyError {
        origin: PemOrigin,
        source: ic_ed25519::PrivateKeyDecodingError,
    },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentity { name: String },

    #[snafu(display("failed to read password: {message}"))]
    GetPasswordError { message: String },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },

    #[snafu(display("failed to load keyring entry"))]
    LoadEntryError { source: keyring::Error },

    #[snafu(display("failed to load password from keyring entry"))]
    LoadPasswordFromEntryError { source: keyring::Error },

    #[snafu(display("failed to load HSM identity"))]
    LoadHsmError {
        source: ic_identity_hsm::HardwareIdentityError,
    },
}

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
        IdentitySpec::Keyring { algorithm, .. } => load_keyring_identity(name, algorithm),
        IdentitySpec::Hsm {
            module,
            slot,
            key_id,
            ..
        } => load_hsm_identity(module, *slot, key_id, password_func),
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
    let origin = PemOrigin::File {
        path: pem_path.clone(),
    };

    let doc = fs::read_to_string(&pem_path)?
        .parse::<Pem>()
        .context(ParsePemSnafu { origin: &origin })?;

    match format {
        PemFormat::Pbes2 => load_pbes2_identity(&doc, algorithm, password_func, &origin),

        PemFormat::Plaintext => load_plaintext_identity(&doc, algorithm, &origin),
    }
}

fn load_pbes2_identity(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    password_func: impl FnOnce() -> Result<String, String>,
    origin: &PemOrigin,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    assert!(
        doc.tag() == pkcs8::EncryptedPrivateKeyInfo::PEM_LABEL,
        "internal error: wrong identity format found"
    );

    let pw = password_func().map_err(|message| LoadIdentityError::GetPasswordError { message })?;

    match algorithm {
        IdentityKeyAlgorithm::Secp256k1 => {
            let key = k256::SecretKey::from_pkcs8_encrypted_der(doc.contents(), &pw)
                .context(ParsePkcs8Snafu { origin })?;

            Ok(Arc::new(Secp256k1Identity::from_private_key(key)))
        }
        IdentityKeyAlgorithm::Prime256v1 => {
            let key = p256::SecretKey::from_pkcs8_encrypted_der(doc.contents(), &pw)
                .context(ParsePkcs8Snafu { origin })?;

            Ok(Arc::new(Prime256v1Identity::from_private_key(key)))
        }
        IdentityKeyAlgorithm::Ed25519 => {
            let encrypted = EncryptedPrivateKeyInfo::from_der(doc.contents())
                .context(ParseDerSnafu { origin })?;
            let decrypted: SecretDocument =
                encrypted.decrypt(&pw).context(ParsePkcs8Snafu { origin })?;
            let key = ic_ed25519::PrivateKey::deserialize_pkcs8(decrypted.as_bytes())
                .context(ParseEd25519KeySnafu { origin })?;
            Ok(Arc::new(BasicIdentity::from_raw_key(&key.serialize_raw())))
        }
    }
}

fn load_plaintext_identity(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    origin: &PemOrigin,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    assert!(
        doc.tag() == PrivateKeyInfo::PEM_LABEL,
        "internal error: wrong identity format found"
    );

    match algorithm {
        IdentityKeyAlgorithm::Secp256k1 => {
            let key = k256::SecretKey::from_pkcs8_der(doc.contents())
                .context(ParsePkcs8Snafu { origin })?;

            Ok(Arc::new(Secp256k1Identity::from_private_key(key)))
        }
        IdentityKeyAlgorithm::Prime256v1 => {
            let key = p256::SecretKey::from_pkcs8_der(doc.contents())
                .context(ParsePkcs8Snafu { origin })?;

            Ok(Arc::new(Prime256v1Identity::from_private_key(key)))
        }
        IdentityKeyAlgorithm::Ed25519 => {
            let key = ic_ed25519::PrivateKey::deserialize_pkcs8(doc.contents())
                .context(ParseEd25519KeySnafu { origin })?;
            Ok(Arc::new(BasicIdentity::from_raw_key(&key.serialize_raw())))
        }
    }
}

const SERVICE_NAME: &str = "icp-cli";

fn load_keyring_identity(
    name: &str,
    algorithm: &IdentityKeyAlgorithm,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let entry = Entry::new(SERVICE_NAME, name).context(LoadEntrySnafu)?;
    let password = entry.get_password().context(LoadPasswordFromEntrySnafu)?;
    let origin = PemOrigin::Keyring {
        service: SERVICE_NAME.to_string(),
        username: name.to_string(),
    };
    let pem = password
        .parse::<Pem>()
        .context(ParsePemSnafu { origin: &origin })?;
    load_plaintext_identity(&pem, algorithm, &origin)
}

#[derive(Debug, Clone)]
pub enum PemOrigin {
    File { path: PathBuf },
    Keyring { service: String, username: String },
}

impl Display for PemOrigin {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PemOrigin::File { path } => write!(f, "file `{path}`"),
            PemOrigin::Keyring { service, username } => {
                let store = if cfg!(target_os = "windows") {
                    "Windows Credential Manager"
                } else if cfg!(target_os = "macos") {
                    "Keychain"
                } else {
                    "secret-service"
                };
                write!(
                    f,
                    "{store} entry (service=`{service}`, username=`{username}`)"
                )
            }
        }
    }
}

impl From<&PemOrigin> for PemOrigin {
    fn from(value: &PemOrigin) -> Self {
        value.clone()
    }
}

fn load_hsm_identity(
    module: &PathBuf,
    slot: usize,
    key_id: &str,
    pin_fn: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let identity = HardwareIdentity::new(module, slot, key_id, pin_fn).context(LoadHsmSnafu)?;

    Ok(Arc::new(identity))
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
    let mut identity_list = IdentityList::load_from(dirs.read())?;
    ensure!(
        !identity_list.identities.contains_key(name),
        IdentityAlreadyExistsSnafu { name }
    );
    let principal = match &key {
        IdentityKey::Secp256k1(secret_key) => {
            Secp256k1Identity::from_private_key(secret_key.clone())
                .sender()
                .expect("infallible method")
        }
        IdentityKey::Prime256v1(secret_key) => {
            Prime256v1Identity::from_private_key(secret_key.clone())
                .sender()
                .expect("infallible method")
        }
        IdentityKey::Ed25519(secret_key) => {
            BasicIdentity::from_raw_key(&secret_key.serialize_raw())
                .sender()
                .expect("infallible method")
        }
    };
    let algorithm = match key {
        IdentityKey::Secp256k1(_) => IdentityKeyAlgorithm::Secp256k1,
        IdentityKey::Prime256v1(_) => IdentityKeyAlgorithm::Prime256v1,
        IdentityKey::Ed25519(_) => IdentityKeyAlgorithm::Ed25519,
    };
    let doc = match key {
        IdentityKey::Secp256k1(key) => key.to_pkcs8_der().expect("infallible PKI encoding"),
        IdentityKey::Prime256v1(key) => key.to_pkcs8_der().expect("infallible PKI encoding"),
        IdentityKey::Ed25519(key) => key
            .serialize_pkcs8(PrivateKeyFormat::Pkcs8v2)
            .try_into()
            .expect("infallible PKI encoding"),
    };
    // store key material
    match &format {
        CreateFormat::Plaintext => {
            let pem = doc
                .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                .expect("infallible PKI encoding");
            write_identity(dirs, name, &pem)?;
        }
        CreateFormat::Pbes2 { password } => {
            let pem = make_pkcs5_encrypted_pem(&doc, password);
            write_identity(dirs, name, &pem)?;
        }
        CreateFormat::Keyring => {
            let pem = doc
                .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                .expect("infallible PKI encoding");
            let entry = Entry::new(SERVICE_NAME, name).context(CreateEntrySnafu)?;
            let res = entry.set_password(&pem);
            #[cfg(target_os = "linux")]
            if let Err(keyring::Error::NoStorageAccess(err)) = &res
                && err.to_string().contains("no result found")
            {
                return NoKeyringSnafu.fail()?;
            }
            res.context(SetEntryPasswordSnafu)?;
        }
    }
    let spec = match format {
        CreateFormat::Plaintext => IdentitySpec::Pem {
            format: PemFormat::Plaintext,
            algorithm,
            principal,
        },
        CreateFormat::Pbes2 { .. } => IdentitySpec::Pem {
            format: PemFormat::Pbes2,
            algorithm,
            principal,
        },
        CreateFormat::Keyring => IdentitySpec::Keyring {
            principal,
            algorithm,
        },
    };
    identity_list.identities.insert(name.to_string(), spec);
    identity_list.write_to(dirs)?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum WriteIdentityError {
    #[snafu(display("failed to write file"))]
    WriteFileError { source: crate::fs::IoError },

    #[snafu(display("failed to create directory"))]
    CreateDirectoryError { source: crate::fs::IoError },

    #[snafu(transparent)]
    LockError { source: crate::fs::lock::LockError },

    #[snafu(display("failed to create keyring entry"))]
    CreateEntryError { source: keyring::Error },
    #[snafu(display("failed to set keyring entry password"))]
    SetEntryPasswordError { source: keyring::Error },
    #[cfg(target_os = "linux")]
    #[snafu(display(
        "no keyring available - have you set it up? gnome-keyring must be installed and configured with a default keyring."
    ))]
    NoKeyring,
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

#[derive(Debug, Snafu)]
pub enum RenameIdentityError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    WriteIdentityManifest { source: WriteIdentityManifestError },

    #[snafu(display("no identity found with name `{name}`"))]
    IdentityNotFound { name: String },

    #[snafu(display("identity `{name}` already exists"))]
    IdentityNameTaken { name: String },

    #[snafu(display("cannot rename the anonymous identity"))]
    CannotRenameAnonymous,

    #[snafu(display("cannot rename to the anonymous identity"))]
    CannotRenameToAnonymous,

    #[snafu(display("failed to copy key file to new location"))]
    CopyKeyFile { source: fs::IoError },

    #[snafu(display("failed to delete old key file"))]
    DeleteOldKeyFile { source: fs::IoError },

    #[snafu(display("failed to load keyring entry for identity `{name}`"))]
    LoadKeyringEntry {
        name: String,
        source: keyring::Error,
    },

    #[snafu(display("failed to read keyring entry for identity `{name}`"))]
    ReadKeyringEntry {
        name: String,
        source: keyring::Error,
    },

    #[snafu(display("failed to create keyring entry for identity `{new_name}`"))]
    CreateKeyringEntry {
        new_name: String,
        source: keyring::Error,
    },

    #[snafu(display("failed to set keyring entry password for identity `{new_name}`"))]
    SetKeyringEntryPassword {
        new_name: String,
        source: keyring::Error,
    },

    #[snafu(display("failed to delete old keyring entry for identity `{old_name}`"))]
    DeleteKeyringEntry {
        old_name: String,
        source: keyring::Error,
    },
}

/// Renames an identity from `old_name` to `new_name`.
///
/// This updates the identity list, renames any PEM files, and updates keyring
/// entries as needed. If the renamed identity was the default, the default is
/// updated to point to the new name.
pub fn rename_identity(
    dirs: LWrite<&IdentityPaths>,
    old_name: &str,
    new_name: &str,
) -> Result<(), RenameIdentityError> {
    // Cannot rename anonymous
    ensure!(old_name != "anonymous", CannotRenameAnonymousSnafu);
    ensure!(new_name != "anonymous", CannotRenameToAnonymousSnafu);

    // Load the identity list
    let mut identity_list = IdentityList::load_from(dirs.read())?;

    // Check the old identity exists
    let spec = identity_list
        .identities
        .remove(old_name)
        .context(IdentityNotFoundSnafu { name: old_name })?;

    // Check the new name doesn't exist
    ensure!(
        !identity_list.identities.contains_key(new_name),
        IdentityNameTakenSnafu { name: new_name }
    );

    // Copy key material to new location before updating the list
    let old_path = dirs.key_pem_path(old_name);
    let old_keyring_entry = match &spec {
        IdentitySpec::Pem { .. } => {
            // Copy the PEM file to the new path
            let new_path = dirs.key_pem_path(new_name);
            let contents = fs::read(&old_path).context(CopyKeyFileSnafu)?;
            fs::write(&new_path, &contents).context(CopyKeyFileSnafu)?;
            None
        }
        IdentitySpec::Keyring { .. } => {
            // Copy the keyring entry to the new name
            let old_entry = Entry::new(SERVICE_NAME, old_name)
                .context(LoadKeyringEntrySnafu { name: old_name })?;
            let password = old_entry
                .get_password()
                .context(ReadKeyringEntrySnafu { name: old_name })?;

            let new_entry =
                Entry::new(SERVICE_NAME, new_name).context(CreateKeyringEntrySnafu { new_name })?;
            new_entry
                .set_password(&password)
                .context(SetKeyringEntryPasswordSnafu { new_name })?;

            Some(old_entry)
        }
        IdentitySpec::Hsm { .. } => {
            // no migration required
            None
        }
        IdentitySpec::Anonymous => {
            unreachable!("anonymous identity should have been rejected above")
        }
    };

    // Update the identity list with the new name
    identity_list.identities.insert(new_name.to_string(), spec);
    identity_list.write_to(dirs)?;

    // Update the default if it was the renamed identity
    let mut defaults = IdentityDefaults::load_from(dirs.read())?;
    if defaults.default == old_name {
        defaults.default = new_name.to_string();
        defaults.write_to(dirs)?;
    }

    // Delete old key material after the list has been updated
    match old_keyring_entry {
        None => {
            // PEM file - delete the old file
            fs::remove_file(&old_path).context(DeleteOldKeyFileSnafu)?;
        }
        Some(entry) => {
            // Keyring - delete the old entry
            entry
                .delete_credential()
                .context(DeleteKeyringEntrySnafu { old_name })?;
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DeleteIdentityError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    WriteIdentityManifest { source: WriteIdentityManifestError },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentityToDelete { name: String },

    #[snafu(display("cannot delete the anonymous identity"))]
    CannotDeleteAnonymous,

    #[snafu(display("cannot delete the default identity `{name}`; change the default first"))]
    CannotDeleteDefault { name: String },

    #[snafu(transparent)]
    DeleteKeyFile { source: fs::IoError },

    #[snafu(display("failed to load keyring entry for identity `{name}`"))]
    LoadKeyringEntryForDelete {
        name: String,
        source: keyring::Error,
    },

    #[snafu(display("failed to delete keyring entry for identity `{name}`"))]
    DeleteKeyringEntryForDelete {
        name: String,
        source: keyring::Error,
    },
}

/// Deletes an identity.
///
/// This removes the identity from the identity list and deletes any associated
/// key files or keyring entries. The anonymous identity and the current default
/// identity cannot be deleted.
pub fn delete_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
) -> Result<(), DeleteIdentityError> {
    // Cannot delete anonymous
    ensure!(name != "anonymous", CannotDeleteAnonymousSnafu);

    // Check if this is the default identity
    let defaults = IdentityDefaults::load_from(dirs.read())?;
    ensure!(defaults.default != name, CannotDeleteDefaultSnafu { name });

    // Load the identity list
    let mut identity_list = IdentityList::load_from(dirs.read())?;

    // Check the identity exists and remove it
    let spec = identity_list
        .identities
        .remove(name)
        .context(NoSuchIdentityToDeleteSnafu { name })?;

    // Save the updated identity list before deleting key material
    identity_list.write_to(dirs)?;

    // Delete key material after the list has been updated
    match &spec {
        IdentitySpec::Pem { .. } => {
            // Delete the PEM file
            let pem_path = dirs.key_pem_path(name);
            fs::remove_file(&pem_path)?;
        }
        IdentitySpec::Keyring { .. } => {
            // Delete the keyring entry
            let entry =
                Entry::new(SERVICE_NAME, name).context(LoadKeyringEntryForDeleteSnafu { name })?;
            entry
                .delete_credential()
                .context(DeleteKeyringEntryForDeleteSnafu { name })?;
        }
        IdentitySpec::Hsm { .. } => {
            // no deletion required
        }
        IdentitySpec::Anonymous => {
            unreachable!("anonymous identity should have been rejected above")
        }
    }

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

#[derive(Debug, Snafu)]
pub enum ExportIdentityError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(display("no identity found with name `{name}`"))]
    NoSuchIdentityToExport { name: String },

    #[snafu(display("cannot export the anonymous identity"))]
    CannotExportAnonymous,

    #[snafu(display("cannot export an HSM-backed identity"))]
    CannotExportHsm,

    #[snafu(display("failed to read PEM file"))]
    ReadPemFileForExport { source: fs::IoError },

    #[snafu(display("failed to parse PEM file"))]
    ParsePemForExport {
        #[snafu(source(from(pem::PemError, Box::new)))]
        source: Box<pem::PemError>,
    },

    #[snafu(display("failed to decrypt PEM file"))]
    DecryptPemForExport { source: pkcs8::Error },

    #[snafu(display("failed to parse decrypted PEM content"))]
    ParseDecryptedForExport { source: pkcs8::der::Error },

    #[snafu(display("failed to read password: {message}"))]
    GetPasswordForExport { message: String },

    #[snafu(display("failed to load keyring entry for identity `{name}`"))]
    LoadKeyringEntryForExport {
        name: String,
        source: keyring::Error,
    },

    #[snafu(display("failed to read keyring entry for identity `{name}`"))]
    ReadKeyringEntryForExport {
        name: String,
        source: keyring::Error,
    },
}

/// Exports an identity as a plaintext PEM string.
///
/// This function loads the identity from either a PEM file or keyring,
/// decrypts it if necessary (prompting for a password), and returns the
/// decrypted PEM string. The anonymous identity cannot be exported.
pub fn export_identity(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<String, ExportIdentityError> {
    // Load the identity list
    let identity_list = IdentityList::load_from(dirs)?;

    // Check the identity exists
    let spec = identity_list
        .identities
        .get(name)
        .context(NoSuchIdentityToExportSnafu { name })?;

    match spec {
        IdentitySpec::Pem { format, .. } => {
            // Read the PEM file
            let pem_path = dirs.key_pem_path(name);
            let pem_contents = fs::read_to_string(&pem_path).context(ReadPemFileForExportSnafu)?;
            let pem = pem_contents
                .parse::<Pem>()
                .context(ParsePemForExportSnafu)?;

            match format {
                PemFormat::Plaintext => {
                    // Already plaintext, return as-is
                    Ok(pem_contents)
                }
                PemFormat::Pbes2 => {
                    // Decrypt the PEM
                    let password = password_func()
                        .map_err(|message| ExportIdentityError::GetPasswordForExport { message })?;

                    // Decrypt to get the plaintext private key info
                    let encrypted = EncryptedPrivateKeyInfo::from_der(pem.contents())
                        .context(ParseDecryptedForExportSnafu)?;
                    let decrypted: SecretDocument = encrypted
                        .decrypt(&password)
                        .context(DecryptPemForExportSnafu)?;

                    // Convert to plaintext PEM string
                    let plaintext_pem = decrypted
                        .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                        .expect("infallible PEM encoding");

                    Ok(plaintext_pem.to_string())
                }
            }
        }
        IdentitySpec::Keyring { .. } => {
            // Read from keyring (already stored as plaintext PEM)
            let entry =
                Entry::new(SERVICE_NAME, name).context(LoadKeyringEntryForExportSnafu { name })?;
            let pem = entry
                .get_password()
                .context(ReadKeyringEntryForExportSnafu { name })?;

            Ok(pem)
        }
        IdentitySpec::Anonymous => CannotExportAnonymousSnafu.fail(),
        IdentitySpec::Hsm { .. } => CannotExportHsmSnafu.fail(),
    }
}
