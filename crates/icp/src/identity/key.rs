use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ic_agent::{
    Identity,
    identity::{
        AnonymousIdentity, BasicIdentity, DelegatedIdentity, Delegation as AgentDelegation,
        DelegationError, Prime256v1Identity, Secp256k1Identity,
    },
};
use ic_ed25519::PrivateKeyFormat;
use ic_identity_hsm::HardwareIdentity;
use keyring::Entry;
use pem::Pem;
use pkcs8::{
    DecodePrivateKey, EncodePrivateKey, EncryptedPrivateKeyInfo, PrivateKeyInfo, SecretDocument,
    pkcs5::pbes2::Parameters,
};
use rand::Rng;
use scrypt::Params;
use sec1::{der::Decode, pem::PemLabel};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use url::Url;
use zeroize::Zeroizing;

use crate::{
    fs::{
        self,
        lock::{LRead, LWrite},
    },
    identity::{
        IdentityPaths,
        delegation::{self, SignedDelegation},
        manifest::{
            DelegationKeyStorage, IdentityDefaults, IdentityKeyAlgorithm, IdentityList,
            IdentitySpec, LoadIdentityManifestError, PemFormat, WriteIdentityManifestError,
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

#[derive(Debug, Clone)]
pub enum ExportFormat {
    Plaintext,
    Encrypted { password: Zeroizing<String> },
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

    #[snafu(display("failed to load delegation chain from `{path}`"))]
    LoadDelegationChain {
        path: PathBuf,
        source: delegation::LoadError,
    },

    #[snafu(display("failed to validate delegation chain loaded from `{path}`"))]
    ValidateDelegationChain {
        path: PathBuf,
        source: DelegationError,
    },

    #[snafu(display(
        "delegation for identity `{name}` has expired or will expire within 5 minutes; \
         run `icp identity login {name}` to re-authenticate"
    ))]
    DelegationExpired { name: String },

    #[snafu(display("failed to convert delegation chain"))]
    DelegationConversion { source: delegation::ConversionError },

    #[snafu(display(
        "identity `{name}` has no delegation yet; \
         run `icp identity delegation use {name}` to complete it"
    ))]
    DelegationNotYetProvided { name: String },
}

pub fn load_identity(
    dirs: LRead<&IdentityPaths>,
    list: &IdentityList,
    name: &str,
    password_func: impl FnOnce() -> Result<String, String>,
    pem_session_duration: Option<Duration>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let identity = list
        .identities
        .get(name)
        .context(NoSuchIdentitySnafu { name })?;

    match identity {
        IdentitySpec::Pem {
            format, algorithm, ..
        } => load_pem_identity(
            dirs,
            name,
            format,
            algorithm,
            password_func,
            pem_session_duration,
        ),
        IdentitySpec::Keyring { algorithm, .. } => load_keyring_identity(name, algorithm),
        IdentitySpec::Hsm {
            module,
            slot,
            key_id,
            ..
        } => load_hsm_identity(module, *slot, key_id, password_func),
        IdentitySpec::Anonymous => Ok(Arc::new(AnonymousIdentity)),
        IdentitySpec::InternetIdentity {
            algorithm, storage, ..
        } => load_ii_identity(dirs, name, algorithm, storage, password_func),
        IdentitySpec::PendingDelegation { .. } => DelegationNotYetProvidedSnafu { name }.fail(),
        IdentitySpec::Delegation {
            algorithm, storage, ..
        } => load_ii_identity(dirs, name, algorithm, storage, password_func),
    }
}

fn load_pem_identity(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    format: &PemFormat,
    algorithm: &IdentityKeyAlgorithm,
    password_func: impl FnOnce() -> Result<String, String>,
    pem_session_duration: Option<Duration>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    // For password-protected PEMs, check for a valid cached session delegation first.
    if *format == PemFormat::Pbes2
        && let Some(id) = try_load_pem_session(dirs, name)
    {
        return Ok(id);
    }

    let pem_path = dirs.key_pem_path(name);
    let origin = PemOrigin::File {
        path: pem_path.clone(),
    };

    let doc = fs::read_to_string(&pem_path)?
        .parse::<Pem>()
        .context(ParsePemSnafu { origin: &origin })?;

    let identity = match format {
        PemFormat::Pbes2 => load_pbes2_identity(&doc, algorithm, password_func, &origin)?,
        PemFormat::Plaintext => load_plaintext_identity(&doc, algorithm, &origin)?,
    };

    // After unlocking a Pbes2 PEM, create and cache a short-lived session delegation.
    if *format == PemFormat::Pbes2
        && let Some(duration) = pem_session_duration
        && let Ok(delegated) =
            create_pem_session_and_build_identity(dirs, name, &*identity, duration)
    {
        return Ok(delegated);
    }

    Ok(identity)
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

/// Returns the keyring username for a delegation session key.
///
/// The `delegation:` prefix discriminates session keys from regular identities —
/// no code path that operates on regular identity names can accidentally
/// export these keys.
fn dlg_keyring_key(name: &str) -> String {
    format!("delegation:{name}")
}

/// Tries to load a previously cached PEM session delegation from keyring + disk.
///
/// Returns `None` on any failure (missing, expired, or keyring unavailable) so the
/// caller falls back to normal PEM loading.
fn try_load_pem_session(dirs: LRead<&IdentityPaths>, name: &str) -> Option<Arc<dyn Identity>> {
    // Load chain from disk; missing file is the common case on first use.
    let chain_path = dirs.delegation_chain_path(name);
    let chain = delegation::load(&chain_path).ok()?;

    if delegation::is_expiring_soon(&chain, FIVE_MINUTES_NANOS).ok()? {
        return None;
    }

    // Load session key from keyring.
    let username = dlg_keyring_key(name);
    let entry = Entry::new(SERVICE_NAME, &username).ok()?;
    let pem_str = entry.get_password().ok()?;
    let origin = PemOrigin::Keyring {
        service: SERVICE_NAME.to_string(),
        username,
    };
    let pem = pem_str.parse::<Pem>().ok()?;
    let session_identity =
        load_plaintext_identity(&pem, &IdentityKeyAlgorithm::Prime256v1, &origin).ok()?;

    let (from_key, signed_delegations) = delegation::to_agent_types(&chain).ok()?;
    DelegatedIdentity::new(from_key, Box::new(session_identity), signed_delegations)
        .ok()
        .map(|id| Arc::new(id) as Arc<dyn Identity>)
}

#[derive(Debug, Snafu)]
pub enum CreateExplicitPemSessionError {
    #[snafu(transparent)]
    ReadFile { source: crate::fs::IoError },

    #[snafu(display("failed to parse PEM from `{path}`"))]
    ParsePemForSession {
        path: PathBuf,
        #[snafu(source(from(pem::PemError, Box::new)))]
        source: Box<pem::PemError>,
    },

    #[snafu(transparent)]
    DecryptPem { source: LoadIdentityError },

    #[snafu(display("failed to sign session delegation: {message}"))]
    SignDelegation { message: String },

    #[snafu(display("failed to create keyring entry for session key"))]
    CreateSessionKeyringEntry { source: keyring::Error },

    #[snafu(display("failed to store session key in keyring"))]
    SetSessionKeyringPassword { source: keyring::Error },

    #[snafu(display("failed to create session delegation directory"))]
    EnsureSessionDelegationDir { source: crate::fs::IoError },

    #[snafu(display("failed to save session delegation chain to `{path}`"))]
    SaveSessionDelegation {
        path: PathBuf,
        source: delegation::SaveError,
    },
}

/// Creates a short-lived P256 session delegation signed by `identity`, stores the session
/// key in the keyring and the chain on disk, and returns a `DelegatedIdentity`.
///
/// Returns an error if any step fails. Automatic callers silence errors via `let Ok(...) =`;
/// explicit callers propagate them.
fn create_pem_session_and_build_identity(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    identity: &dyn Identity,
    duration: std::time::Duration,
) -> Result<Arc<dyn Identity>, CreateExplicitPemSessionError> {
    let signer_pubkey = identity
        .public_key()
        .expect("called only with non-anonymous identity");

    let mut key_bytes = Zeroizing::new([0u8; 32]);
    rand::rng().fill_bytes(key_bytes.as_mut());
    let session_key = p256::SecretKey::from_slice(&key_bytes[..])
        .expect("random 32 bytes are a valid p256 scalar");
    let session_identity = Prime256v1Identity::from_private_key(session_key.clone());
    let session_pubkey = session_identity
        .public_key()
        .expect("p256 always has a public key");

    let now_nanos = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos(),
    )
    .expect("time wrapped around");
    let expiration =
        now_nanos.saturating_add(duration.as_nanos().try_into().expect("time wrapped around"));

    let agent_delegation = AgentDelegation {
        pubkey: session_pubkey.clone(),
        expiration,
        targets: None,
    };

    let sig = identity
        .sign_delegation(&agent_delegation)
        .map_err(|message| CreateExplicitPemSessionError::SignDelegation { message })?;
    let signature_bytes = sig
        .signature
        .expect("non-anonymous identity always produces a signature");

    // Walk any existing delegation chain (e.g. if `identity` is already delegated),
    // then append the new delegation to the session key.
    let mut wire_delegations: Vec<delegation::SignedDelegation> = sig
        .delegations
        .unwrap_or_default()
        .into_iter()
        .map(|sd| delegation::SignedDelegation {
            signature: hex::encode(&sd.signature),
            delegation: delegation::Delegation {
                pubkey: hex::encode(&sd.delegation.pubkey),
                expiration: format!("{:x}", sd.delegation.expiration),
                targets: sd
                    .delegation
                    .targets
                    .as_ref()
                    .map(|ts| ts.iter().map(|p| hex::encode(p.as_slice())).collect()),
            },
        })
        .collect();

    wire_delegations.push(SignedDelegation {
        signature: hex::encode(&signature_bytes),
        delegation: delegation::Delegation {
            pubkey: hex::encode(&session_pubkey),
            expiration: format!("{expiration:x}"),
            targets: None,
        },
    });

    let chain = delegation::DelegationChain {
        public_key: hex::encode(&signer_pubkey),
        delegations: wire_delegations,
    };

    // Store session key in keyring.
    let doc = session_key.to_pkcs8_der().expect("infallible PKI encoding");
    let pem_str: Zeroizing<String> = doc
        .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
        .expect("infallible PKI encoding");
    let entry =
        Entry::new(SERVICE_NAME, &dlg_keyring_key(name)).context(CreateSessionKeyringEntrySnafu)?;
    entry
        .set_password(&pem_str)
        .context(SetSessionKeyringPasswordSnafu)?;

    // Store chain on disk; on failure undo the keyring entry.
    let chain_path = (*dirs)
        .ensure_delegation_chain_path(name)
        .map_err(|source| {
            let _ = entry.delete_credential();
            CreateExplicitPemSessionError::EnsureSessionDelegationDir { source }
        })?;
    delegation::save(&chain_path, &chain).map_err(|source| {
        let _ = entry.delete_credential();
        CreateExplicitPemSessionError::SaveSessionDelegation {
            path: chain_path.clone(),
            source,
        }
    })?;

    // Build identity directly from in-memory data — no read-back needed.
    let (from_key, signed_delegations) =
        delegation::to_agent_types(&chain).expect("freshly created chain is always valid");
    let session_arc: Arc<dyn Identity> = Arc::new(session_identity);
    let delegated = DelegatedIdentity::new(from_key, Box::new(session_arc), signed_delegations)
        .expect("freshly created chain is always valid");
    Ok(Arc::new(delegated) as Arc<dyn Identity>)
}

/// Creates a PEM session delegation explicitly, prompting for the password and storing
/// the new session key and chain.
///
/// Unlike the automatic path (which silences errors), this propagates them.
/// The `duration` should already include the 5-minute clock-drift boost.
pub fn create_explicit_pem_session(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    algorithm: &IdentityKeyAlgorithm,
    password_func: impl FnOnce() -> Result<String, String>,
    duration: Duration,
) -> Result<(), CreateExplicitPemSessionError> {
    let pem_path = dirs.key_pem_path(name);
    let origin = PemOrigin::File {
        path: pem_path.clone(),
    };

    let doc = fs::read_to_string(&pem_path)?
        .parse::<Pem>()
        .context(ParsePemForSessionSnafu { path: &pem_path })?;

    let identity = load_pbes2_identity(&doc, algorithm, password_func, &origin)?;

    create_pem_session_and_build_identity(dirs, name, &*identity, duration)?;
    Ok(())
}

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

const FIVE_MINUTES_NANOS: u64 = 5 * 60 * 1_000_000_000;

fn load_ii_identity(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    algorithm: &IdentityKeyAlgorithm,
    storage: &DelegationKeyStorage,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let (doc, origin) = load_ii_session_pem(dirs, name, storage)?;

    // Load the delegation chain
    let chain_path = dirs.delegation_chain_path(name);
    let stored_chain =
        delegation::load(&chain_path).context(LoadDelegationChainSnafu { path: &chain_path })?;

    // Check expiry (5 minutes grace)
    if delegation::is_expiring_soon(&stored_chain, FIVE_MINUTES_NANOS)
        .context(DelegationConversionSnafu)?
    {
        return DelegationExpiredSnafu { name }.fail();
    }

    // Convert hex-encoded wire format to ic-agent types
    let (from_key, signed_delegations) =
        delegation::to_agent_types(&stored_chain).context(DelegationConversionSnafu)?;

    let inner: Arc<dyn Identity> = match storage {
        DelegationKeyStorage::Keyring
        | DelegationKeyStorage::Pem {
            format: PemFormat::Plaintext,
        } => load_plaintext_identity(&doc, algorithm, &origin)?,
        DelegationKeyStorage::Pem {
            format: PemFormat::Pbes2,
        } => load_pbes2_identity(&doc, algorithm, password_func, &origin)?,
    };

    let delegated = DelegatedIdentity::new(from_key, Box::new(inner), signed_delegations)
        .context(ValidateDelegationChainSnafu { path: &chain_path })?;

    Ok(Arc::new(delegated))
}

/// Returns the DER-encoded public key for a stored II session key.
///
/// Used during re-authentication to obtain the session public key without
/// re-loading the full delegated identity.
pub fn load_ii_session_public_key(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    algorithm: &IdentityKeyAlgorithm,
    storage: &DelegationKeyStorage,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<Vec<u8>, LoadIdentityError> {
    let (doc, origin) = load_ii_session_pem(dirs, name, storage)?;

    match storage {
        DelegationKeyStorage::Keyring
        | DelegationKeyStorage::Pem {
            format: PemFormat::Plaintext,
        } => load_ii_public_key_plaintext(&doc, algorithm, &origin),
        DelegationKeyStorage::Pem {
            format: PemFormat::Pbes2,
        } => {
            let pw = password_func()
                .map_err(|message| LoadIdentityError::GetPasswordError { message })?;
            load_ii_public_key_pbes2(&doc, algorithm, &origin, &pw)
        }
    }
}

fn load_ii_session_pem(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    storage: &DelegationKeyStorage,
) -> Result<(Pem, PemOrigin), LoadIdentityError> {
    match storage {
        DelegationKeyStorage::Keyring => {
            let username = dlg_keyring_key(name);
            let entry = Entry::new(SERVICE_NAME, &username).context(LoadEntrySnafu)?;
            let pem_str = entry.get_password().context(LoadPasswordFromEntrySnafu)?;
            let origin = PemOrigin::Keyring {
                service: SERVICE_NAME.to_string(),
                username,
            };
            let doc = pem_str
                .parse::<Pem>()
                .context(ParsePemSnafu { origin: &origin })?;
            Ok((doc, origin))
        }
        DelegationKeyStorage::Pem { .. } => {
            let pem_path = dirs.key_pem_path(name);
            let origin = PemOrigin::File {
                path: pem_path.clone(),
            };
            let doc = fs::read_to_string(&pem_path)?
                .parse::<Pem>()
                .context(ParsePemSnafu { origin: &origin })?;
            Ok((doc, origin))
        }
    }
}

fn load_ii_public_key_plaintext(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    origin: &PemOrigin,
) -> Result<Vec<u8>, LoadIdentityError> {
    match algorithm {
        IdentityKeyAlgorithm::Ed25519 => {
            let key = ic_ed25519::PrivateKey::deserialize_pkcs8(doc.contents())
                .context(ParseEd25519KeySnafu { origin })?;
            Ok(BasicIdentity::from_raw_key(&key.serialize_raw())
                .public_key()
                .expect("ed25519 always has a public key"))
        }
        IdentityKeyAlgorithm::Secp256k1 => {
            let key = k256::SecretKey::from_pkcs8_der(doc.contents())
                .context(ParsePkcs8Snafu { origin })?;
            Ok(Secp256k1Identity::from_private_key(key)
                .public_key()
                .expect("secp256k1 always has a public key"))
        }
        IdentityKeyAlgorithm::Prime256v1 => {
            let key = p256::SecretKey::from_pkcs8_der(doc.contents())
                .context(ParsePkcs8Snafu { origin })?;
            Ok(Prime256v1Identity::from_private_key(key)
                .public_key()
                .expect("p256 always has a public key"))
        }
    }
}

fn load_ii_public_key_pbes2(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    origin: &PemOrigin,
    pw: &str,
) -> Result<Vec<u8>, LoadIdentityError> {
    match algorithm {
        IdentityKeyAlgorithm::Ed25519 => {
            let encrypted = EncryptedPrivateKeyInfo::from_der(doc.contents())
                .context(ParseDerSnafu { origin })?;
            let decrypted: SecretDocument =
                encrypted.decrypt(pw).context(ParsePkcs8Snafu { origin })?;
            let key = ic_ed25519::PrivateKey::deserialize_pkcs8(decrypted.as_bytes())
                .context(ParseEd25519KeySnafu { origin })?;
            Ok(BasicIdentity::from_raw_key(&key.serialize_raw())
                .public_key()
                .expect("ed25519 always has a public key"))
        }
        IdentityKeyAlgorithm::Secp256k1 => {
            let key = k256::SecretKey::from_pkcs8_encrypted_der(doc.contents(), pw)
                .context(ParsePkcs8Snafu { origin })?;
            Ok(Secp256k1Identity::from_private_key(key)
                .public_key()
                .expect("secp256k1 always has a public key"))
        }
        IdentityKeyAlgorithm::Prime256v1 => {
            let key = p256::SecretKey::from_pkcs8_encrypted_der(doc.contents(), pw)
                .context(ParsePkcs8Snafu { origin })?;
            Ok(Prime256v1Identity::from_private_key(key)
                .public_key()
                .expect("p256 always has a public key"))
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
    pem_session_duration: Option<Duration>,
) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
    let identity = load_identity(
        dirs,
        &IdentityList::load_from(dirs)?,
        &(IdentityDefaults::load_from(dirs)?).default,
        password_func,
        pem_session_duration,
    )?;

    Ok(identity)
}

pub const MIN_IDENTITY_PASSWORD_LEN: usize = 8;

pub fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < MIN_IDENTITY_PASSWORD_LEN {
        return Err(format!(
            "password must be at least {} characters",
            MIN_IDENTITY_PASSWORD_LEN
        ));
    }
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
    enum OldKeyMaterial {
        Pem(PathBuf),
        Keyring(Entry),
        DelegationKeyring(Entry),
        DelegationPem(PathBuf),
        IiKeyringAndDelegation(Entry, PathBuf),
        IiPemAndDelegation(PathBuf, PathBuf),
        None,
    }

    let old_key_material = match &spec {
        IdentitySpec::Pem { .. } => {
            // Copy the PEM file to the new path
            let old_path = dirs.key_pem_path(old_name);
            let new_path = dirs.key_pem_path(new_name);
            let contents = fs::read(&old_path).context(CopyKeyFileSnafu)?;
            fs::write(&new_path, &contents).context(CopyKeyFileSnafu)?;

            // Best-effort: migrate any cached session delegation.
            if let Ok(old_entry) = Entry::new(SERVICE_NAME, &dlg_keyring_key(old_name))
                && let Ok(pem_str) = old_entry.get_password()
            {
                if let Ok(new_entry) = Entry::new(SERVICE_NAME, &dlg_keyring_key(new_name)) {
                    let _ = new_entry.set_password(&pem_str);
                }
                let _ = old_entry.delete_credential();
            }
            let old_chain_path = dirs.delegation_chain_path(old_name);
            if let Ok(chain_bytes) = fs::read(&old_chain_path)
                && let Ok(new_chain_path) = (*dirs).ensure_delegation_chain_path(new_name)
            {
                let _ = fs::write(&new_chain_path, &chain_bytes);
                let _ = fs::remove_file(&old_chain_path);
            }

            OldKeyMaterial::Pem(old_path)
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

            OldKeyMaterial::Keyring(old_entry)
        }
        IdentitySpec::InternetIdentity { storage, .. } => {
            let old_delegation = dirs.delegation_chain_path(old_name);
            let new_delegation = dirs
                .ensure_delegation_chain_path(new_name)
                .context(CopyKeyFileSnafu)?;
            let delegation_contents = fs::read(&old_delegation).context(CopyKeyFileSnafu)?;
            fs::write(&new_delegation, &delegation_contents).context(CopyKeyFileSnafu)?;

            match storage {
                DelegationKeyStorage::Keyring => {
                    let old_entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(old_name))
                        .context(LoadKeyringEntrySnafu { name: old_name })?;
                    let password = old_entry
                        .get_password()
                        .context(ReadKeyringEntrySnafu { name: old_name })?;
                    let new_entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(new_name))
                        .context(CreateKeyringEntrySnafu { new_name })?;
                    new_entry
                        .set_password(&password)
                        .context(SetKeyringEntryPasswordSnafu { new_name })?;
                    OldKeyMaterial::IiKeyringAndDelegation(old_entry, old_delegation)
                }
                DelegationKeyStorage::Pem { .. } => {
                    let old_pem = dirs.key_pem_path(old_name);
                    let new_pem = dirs.key_pem_path(new_name);
                    let contents = fs::read(&old_pem).context(CopyKeyFileSnafu)?;
                    fs::write(&new_pem, &contents).context(CopyKeyFileSnafu)?;
                    OldKeyMaterial::IiPemAndDelegation(old_pem, old_delegation)
                }
            }
        }
        IdentitySpec::Hsm { .. } => {
            // No migration required - HSM key stays on device
            OldKeyMaterial::None
        }
        IdentitySpec::Anonymous => {
            unreachable!("anonymous identity should have been rejected above")
        }
        IdentitySpec::PendingDelegation { storage, .. } => match storage {
            DelegationKeyStorage::Keyring => {
                let old_entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(old_name))
                    .context(LoadKeyringEntrySnafu { name: old_name })?;
                let password = old_entry
                    .get_password()
                    .context(ReadKeyringEntrySnafu { name: old_name })?;
                let new_entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(new_name))
                    .context(CreateKeyringEntrySnafu { new_name })?;
                new_entry
                    .set_password(&password)
                    .context(SetKeyringEntryPasswordSnafu { new_name })?;
                OldKeyMaterial::DelegationKeyring(old_entry)
            }
            DelegationKeyStorage::Pem { .. } => {
                let old_pem = dirs.key_pem_path(old_name);
                let new_pem = dirs.key_pem_path(new_name);
                let contents = fs::read(&old_pem).context(CopyKeyFileSnafu)?;
                fs::write(&new_pem, &contents).context(CopyKeyFileSnafu)?;
                OldKeyMaterial::DelegationPem(old_pem)
            }
        },
        IdentitySpec::Delegation { storage, .. } => {
            let old_delegation = dirs.delegation_chain_path(old_name);
            let new_delegation = dirs
                .ensure_delegation_chain_path(new_name)
                .context(CopyKeyFileSnafu)?;
            let delegation_contents = fs::read(&old_delegation).context(CopyKeyFileSnafu)?;
            fs::write(&new_delegation, &delegation_contents).context(CopyKeyFileSnafu)?;

            match storage {
                DelegationKeyStorage::Keyring => {
                    let old_entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(old_name))
                        .context(LoadKeyringEntrySnafu { name: old_name })?;
                    let password = old_entry
                        .get_password()
                        .context(ReadKeyringEntrySnafu { name: old_name })?;
                    let new_entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(new_name))
                        .context(CreateKeyringEntrySnafu { new_name })?;
                    new_entry
                        .set_password(&password)
                        .context(SetKeyringEntryPasswordSnafu { new_name })?;
                    OldKeyMaterial::IiKeyringAndDelegation(old_entry, old_delegation)
                }
                DelegationKeyStorage::Pem { .. } => {
                    let old_pem = dirs.key_pem_path(old_name);
                    let new_pem = dirs.key_pem_path(new_name);
                    let contents = fs::read(&old_pem).context(CopyKeyFileSnafu)?;
                    fs::write(&new_pem, &contents).context(CopyKeyFileSnafu)?;
                    OldKeyMaterial::IiPemAndDelegation(old_pem, old_delegation)
                }
            }
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
    match old_key_material {
        OldKeyMaterial::Pem(old_path) => {
            fs::remove_file(&old_path).context(DeleteOldKeyFileSnafu)?;
        }
        OldKeyMaterial::Keyring(entry) => {
            entry
                .delete_credential()
                .context(DeleteKeyringEntrySnafu { old_name })?;
        }
        OldKeyMaterial::DelegationKeyring(old_entry) => {
            old_entry
                .delete_credential()
                .context(DeleteKeyringEntrySnafu { old_name })?;
        }
        OldKeyMaterial::DelegationPem(old_pem) => {
            fs::remove_file(&old_pem).context(DeleteOldKeyFileSnafu)?;
        }
        OldKeyMaterial::IiKeyringAndDelegation(old_entry, old_delegation) => {
            old_entry
                .delete_credential()
                .context(DeleteKeyringEntrySnafu { old_name })?;
            fs::remove_file(&old_delegation).context(DeleteOldKeyFileSnafu)?;
        }
        OldKeyMaterial::IiPemAndDelegation(old_pem, old_delegation) => {
            fs::remove_file(&old_pem).context(DeleteOldKeyFileSnafu)?;
            fs::remove_file(&old_delegation).context(DeleteOldKeyFileSnafu)?;
        }
        OldKeyMaterial::None => {
            // Nothing to clean up (HSM identities)
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
            // Best-effort: clean up any cached session delegation.
            if let Ok(entry) = Entry::new(SERVICE_NAME, &dlg_keyring_key(name)) {
                let _ = entry.delete_credential();
            }
            let _ = fs::remove_file(&dirs.delegation_chain_path(name));
        }
        IdentitySpec::Keyring { .. } => {
            // Delete the keyring entry
            let entry =
                Entry::new(SERVICE_NAME, name).context(LoadKeyringEntryForDeleteSnafu { name })?;
            entry
                .delete_credential()
                .context(DeleteKeyringEntryForDeleteSnafu { name })?;
        }
        IdentitySpec::InternetIdentity { storage, .. } => {
            match storage {
                DelegationKeyStorage::Keyring => {
                    let entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(name))
                        .context(LoadKeyringEntryForDeleteSnafu { name })?;
                    entry
                        .delete_credential()
                        .context(DeleteKeyringEntryForDeleteSnafu { name })?;
                }
                DelegationKeyStorage::Pem { .. } => {
                    let pem_path = dirs.key_pem_path(name);
                    fs::remove_file(&pem_path)?;
                }
            }
            let delegation_path = dirs.delegation_chain_path(name);
            fs::remove_file(&delegation_path)?;
        }
        IdentitySpec::Hsm { .. } => {
            // no deletion required
        }
        IdentitySpec::Anonymous => {
            unreachable!("anonymous identity should have been rejected above")
        }
        IdentitySpec::PendingDelegation { storage, .. } => match storage {
            DelegationKeyStorage::Keyring => {
                let entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(name))
                    .context(LoadKeyringEntryForDeleteSnafu { name })?;
                entry
                    .delete_credential()
                    .context(DeleteKeyringEntryForDeleteSnafu { name })?;
            }
            DelegationKeyStorage::Pem { .. } => {
                let pem_path = dirs.key_pem_path(name);
                fs::remove_file(&pem_path)?;
            }
        },
        IdentitySpec::Delegation { storage, .. } => {
            match storage {
                DelegationKeyStorage::Keyring => {
                    let entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(name))
                        .context(LoadKeyringEntryForDeleteSnafu { name })?;
                    entry
                        .delete_credential()
                        .context(DeleteKeyringEntryForDeleteSnafu { name })?;
                }
                DelegationKeyStorage::Pem { .. } => {
                    let pem_path = dirs.key_pem_path(name);
                    fs::remove_file(&pem_path)?;
                }
            }
            let delegation_path = dirs.delegation_chain_path(name);
            fs::remove_file(&delegation_path)?;
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum LinkHsmIdentityError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    WriteIdentityManifest { source: WriteIdentityManifestError },

    #[snafu(display("identity `{name}` already exists"))]
    NameTaken { name: String },

    #[snafu(display("failed to connect to HSM"))]
    HsmConnection {
        source: ic_identity_hsm::HardwareIdentityError,
    },
}

/// Links an HSM key slot to a named identity.
///
/// This creates an identity that references a key stored on a hardware security
/// module (HSM) like a YubiKey. The private key never leaves the device.
pub fn link_hsm_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    module: PathBuf,
    slot: usize,
    key_id: String,
    pin_func: impl FnOnce() -> Result<String, String>,
) -> Result<(), LinkHsmIdentityError> {
    let mut identity_list = IdentityList::load_from(dirs.read())?;
    ensure!(
        !identity_list.identities.contains_key(name),
        NameTakenSnafu { name }
    );

    // Connect to the HSM to verify the parameters and get the principal
    let identity =
        HardwareIdentity::new(&module, slot, &key_id, pin_func).context(HsmConnectionSnafu)?;
    let principal = identity.sender().expect("infallible method");

    let spec = IdentitySpec::Hsm {
        principal,
        module,
        slot,
        key_id,
    };
    identity_list.identities.insert(name.to_string(), spec);
    identity_list.write_to(dirs)?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CreatePendingDelegationError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    WriteIdentityManifest { source: WriteIdentityManifestError },

    #[snafu(display("identity `{name}` already exists"))]
    DlgNameTaken { name: String },

    #[snafu(display("failed to create session key keyring entry"))]
    DlgCreateKeyringEntry { source: keyring::Error },

    #[snafu(display("failed to store session key in keyring"))]
    DlgSetKeyringEntryPassword { source: keyring::Error },

    #[cfg(target_os = "linux")]
    #[snafu(display(
        "no keyring available - have you set it up? gnome-keyring must be installed and configured with a default keyring."
    ))]
    DlgNoKeyring,

    #[snafu(display("failed to write session key PEM file for `{name}`"))]
    DlgWritePemFile {
        name: String,
        source: crate::fs::IoError,
    },

    #[snafu(display("failed to create delegation directory"))]
    DlgCreateDelegationDir { source: crate::fs::IoError },

    #[snafu(display("failed to save delegation chain to `{path}`"))]
    DlgSaveDelegation {
        path: PathBuf,
        source: delegation::SaveError,
    },
}

/// Links an Internet Identity delegation to a new named identity.
///
/// Stores the session keypair according to `storage` and the delegation chain
/// as a separate JSON file.
pub fn link_ii_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    key: IdentityKey,
    chain: &delegation::DelegationChain,
    principal: ic_agent::export::Principal,
    create_format: CreateFormat,
    host: Url,
) -> Result<(), CreatePendingDelegationError> {
    let mut identity_list = IdentityList::load_from(dirs.read())?;
    ensure!(
        !identity_list.identities.contains_key(name),
        DlgNameTakenSnafu { name }
    );

    let algorithm = match &key {
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

    let ii_storage = match &create_format {
        CreateFormat::Keyring => {
            let pem = doc
                .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                .expect("infallible PKI encoding");
            let entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(name))
                .context(DlgCreateKeyringEntrySnafu)?;
            let res = entry.set_password(&pem);
            #[cfg(target_os = "linux")]
            if let Err(keyring::Error::NoStorageAccess(err)) = &res
                && err.to_string().contains("no result found")
            {
                return DlgNoKeyringSnafu.fail()?;
            }
            res.context(DlgSetKeyringEntryPasswordSnafu)?;
            DelegationKeyStorage::Keyring
        }
        CreateFormat::Plaintext => {
            let pem = doc
                .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                .expect("infallible PKI encoding");
            let pem_path = dirs
                .ensure_key_pem_path(name)
                .context(DlgWritePemFileSnafu { name })?;
            fs::write_string(&pem_path, &pem).context(DlgWritePemFileSnafu { name })?;
            DelegationKeyStorage::Pem {
                format: PemFormat::Plaintext,
            }
        }
        CreateFormat::Pbes2 { password } => {
            let pem = make_pkcs5_encrypted_pem(&doc, password.as_str());
            let pem_path = dirs
                .ensure_key_pem_path(name)
                .context(DlgWritePemFileSnafu { name })?;
            fs::write_string(&pem_path, &pem).context(DlgWritePemFileSnafu { name })?;
            DelegationKeyStorage::Pem {
                format: PemFormat::Pbes2,
            }
        }
    };

    let delegation_path = dirs
        .ensure_delegation_chain_path(name)
        .context(DlgCreateDelegationDirSnafu)?;
    delegation::save(&delegation_path, chain).context(DlgSaveDelegationSnafu {
        path: &delegation_path,
    })?;

    let spec = IdentitySpec::InternetIdentity {
        algorithm,
        principal,
        storage: ii_storage,
        host,
    };
    identity_list.identities.insert(name.to_string(), spec);
    identity_list.write_to(dirs)?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum UpdateIiDelegationError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(display("no identity found with name `{name}`"))]
    IiIdentityNotFound { name: String },

    #[snafu(display("identity `{name}` is not an Internet Identity"))]
    NotInternetIdentity { name: String },

    #[snafu(display("failed to save delegation chain to `{path}`"))]
    UpdateIiDelegationSave {
        path: PathBuf,
        source: delegation::SaveError,
    },

    #[snafu(display("failed to create delegation directory"))]
    UpdateIiCreateDir { source: crate::fs::IoError },
}

/// Updates the delegation chain for an existing Internet Identity.
pub fn update_ii_delegation(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    chain: &delegation::DelegationChain,
) -> Result<(), UpdateIiDelegationError> {
    let identity_list = IdentityList::load_from(dirs.read())?;
    let spec = identity_list
        .identities
        .get(name)
        .context(IiIdentityNotFoundSnafu { name })?;

    ensure!(
        matches!(spec, IdentitySpec::InternetIdentity { .. }),
        NotInternetIdentitySnafu { name }
    );

    let delegation_path = dirs
        .ensure_delegation_chain_path(name)
        .context(UpdateIiCreateDirSnafu)?;
    delegation::save(&delegation_path, chain).context(UpdateIiDelegationSaveSnafu {
        path: &delegation_path,
    })?;

    Ok(())
}

/// Creates a new pending delegation identity with a fresh P256 session key.
///
/// Stores the session key according to `create_format` and registers the identity
/// as `PendingDelegation`. Returns the DER-encoded SPKI public key to hand to a
/// signer via `icp identity delegation sign --key-pem`.
pub fn create_pending_delegation(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    create_format: CreateFormat,
) -> Result<Vec<u8>, CreatePendingDelegationError> {
    let mut identity_list = IdentityList::load_from(dirs.read())?;
    ensure!(
        !identity_list.identities.contains_key(name),
        DlgNameTakenSnafu { name }
    );

    let mut key_bytes = Zeroizing::new([0u8; 32]);
    rand::rng().fill_bytes(key_bytes.as_mut());
    let key = p256::SecretKey::from_slice(&key_bytes[..])
        .expect("random 32 bytes are a valid p256 scalar");
    let identity = Prime256v1Identity::from_private_key(key.clone());
    let der_public_key = identity.public_key().expect("p256 always has a public key");

    let doc = key.to_pkcs8_der().expect("infallible PKI encoding");

    let storage = match &create_format {
        CreateFormat::Keyring => {
            let pem = doc
                .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                .expect("infallible PKI encoding");
            let entry = Entry::new(SERVICE_NAME, &dlg_keyring_key(name))
                .context(DlgCreateKeyringEntrySnafu)?;
            let res = entry.set_password(&pem);
            #[cfg(target_os = "linux")]
            if let Err(keyring::Error::NoStorageAccess(err)) = &res
                && err.to_string().contains("no result found")
            {
                return DlgNoKeyringSnafu.fail()?;
            }
            res.context(DlgSetKeyringEntryPasswordSnafu)?;
            DelegationKeyStorage::Keyring
        }
        CreateFormat::Plaintext => {
            let pem = doc
                .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                .expect("infallible PKI encoding");
            let pem_path = dirs
                .ensure_key_pem_path(name)
                .context(DlgWritePemFileSnafu { name })?;
            fs::write_string(&pem_path, &pem).context(DlgWritePemFileSnafu { name })?;
            DelegationKeyStorage::Pem {
                format: PemFormat::Plaintext,
            }
        }
        CreateFormat::Pbes2 { password } => {
            let pem = make_pkcs5_encrypted_pem(&doc, password.as_str());
            let pem_path = dirs
                .ensure_key_pem_path(name)
                .context(DlgWritePemFileSnafu { name })?;
            fs::write_string(&pem_path, &pem).context(DlgWritePemFileSnafu { name })?;
            DelegationKeyStorage::Pem {
                format: PemFormat::Pbes2,
            }
        }
    };

    let spec = IdentitySpec::PendingDelegation {
        algorithm: IdentityKeyAlgorithm::Prime256v1,
        storage,
    };
    identity_list.identities.insert(name.to_string(), spec);
    identity_list.write_to(dirs)?;

    Ok(der_public_key)
}

#[derive(Debug, Snafu)]
pub enum CompleteDelegationError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(transparent)]
    WriteIdentityManifest { source: WriteIdentityManifestError },

    #[snafu(display("no identity found with name `{name}`"))]
    DelegationIdentityNotFound { name: String },

    #[snafu(display("identity `{name}` is not a pending delegation"))]
    IdentityNotPending { name: String },

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeDelegationChainKey { source: hex::FromHexError },

    #[snafu(display("failed to create delegation directory"))]
    CreateDelegationChainDir { source: crate::fs::IoError },

    #[snafu(display("failed to save delegation chain to `{path}`"))]
    SaveDelegationChain {
        path: PathBuf,
        source: delegation::SaveError,
    },
}

/// Completes a `PendingDelegation` identity by attaching a signed delegation chain.
///
/// Updates the identity spec to `Delegation` with the root principal derived from
/// `chain.public_key`. After this call the identity is usable for signing.
/// Returns the storage mode so callers can warn about plaintext storage.
pub fn complete_delegation(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    chain: &delegation::DelegationChain,
) -> Result<DelegationKeyStorage, CompleteDelegationError> {
    let mut identity_list = IdentityList::load_from(dirs.read())?;
    let spec = identity_list
        .identities
        .get(name)
        .context(DelegationIdentityNotFoundSnafu { name })?;

    let (algorithm, storage) = match spec {
        IdentitySpec::PendingDelegation { algorithm, storage } => (algorithm.clone(), *storage),
        _ => return IdentityNotPendingSnafu { name }.fail(),
    };

    let from_key = hex::decode(&chain.public_key).context(DecodeDelegationChainKeySnafu)?;
    let principal = ic_agent::export::Principal::self_authenticating(&from_key);

    let delegation_path = dirs
        .ensure_delegation_chain_path(name)
        .context(CreateDelegationChainDirSnafu)?;
    delegation::save(&delegation_path, chain).context(SaveDelegationChainSnafu {
        path: &delegation_path,
    })?;

    let new_spec = IdentitySpec::Delegation {
        algorithm,
        principal,
        storage,
    };
    identity_list.identities.insert(name.to_string(), new_spec);
    identity_list.write_to(dirs)?;

    Ok(storage)
}

fn encrypt_pki(pki: &PrivateKeyInfo<'_>, password: &str) -> Zeroizing<String> {
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

fn make_pkcs5_encrypted_pem(doc: &SecretDocument, password: &str) -> Zeroizing<String> {
    let pki = PrivateKeyInfo::from_der(doc.as_bytes()).expect("infallible PKI roundtrip");
    encrypt_pki(&pki, password)
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

    #[snafu(display("cannot export an Internet Identity-backed identity"))]
    CannotExportInternetIdentity,

    #[snafu(display("cannot export a delegation identity"))]
    CannotExportDelegation,

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

    #[snafu(display("{message}"))]
    BadPassword { message: String },
}

/// Exports an identity as a PEM string, optionally encrypted.
///
/// This function loads the identity from either a PEM file or keyring,
/// decrypts it if necessary (prompting for a password via `password_func`),
/// and returns the PEM string in the requested [`ExportFormat`].
pub fn export_identity(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    export_format: ExportFormat,
    password_func: impl FnOnce() -> Result<String, String>,
) -> Result<String, ExportIdentityError> {
    if let ExportFormat::Encrypted { password } = &export_format {
        validate_password(password)
            .map_err(|message| ExportIdentityError::BadPassword { message })?;
    }

    // Load the identity list
    let identity_list = IdentityList::load_from(dirs)?;

    // Check the identity exists
    let spec = identity_list
        .identities
        .get(name)
        .context(NoSuchIdentityToExportSnafu { name })?;

    let plaintext_pem = match spec {
        IdentitySpec::Pem {
            format: storage_format,
            ..
        } => {
            // Read the PEM file
            let pem_path = dirs.key_pem_path(name);
            let pem_contents = fs::read_to_string(&pem_path).context(ReadPemFileForExportSnafu)?;
            let pem = pem_contents
                .parse::<Pem>()
                .context(ParsePemForExportSnafu)?;

            match storage_format {
                // Already plaintext, return as-is
                PemFormat::Plaintext => pem_contents,
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
                    decrypted
                        .to_pem(PrivateKeyInfo::PEM_LABEL, Default::default())
                        .expect("infallible PEM encoding")
                        .to_string()
                }
            }
        }
        IdentitySpec::Keyring { .. } => {
            // Read from keyring (already stored as plaintext PEM)
            let entry =
                Entry::new(SERVICE_NAME, name).context(LoadKeyringEntryForExportSnafu { name })?;
            entry
                .get_password()
                .context(ReadKeyringEntryForExportSnafu { name })?
        }
        IdentitySpec::Anonymous => return CannotExportAnonymousSnafu.fail(),
        IdentitySpec::Hsm { .. } => return CannotExportHsmSnafu.fail(),
        IdentitySpec::InternetIdentity { .. } => return CannotExportInternetIdentitySnafu.fail(),
        IdentitySpec::PendingDelegation { .. } | IdentitySpec::Delegation { .. } => {
            return CannotExportDelegationSnafu.fail();
        }
    };

    match export_format {
        ExportFormat::Plaintext => Ok(plaintext_pem),
        ExportFormat::Encrypted { password } => {
            let pem: Pem = plaintext_pem
                .parse()
                .expect("internal error: exported PEM is invalid");
            let pki = PrivateKeyInfo::from_der(pem.contents())
                .expect("internal error: exported key is not valid PKCS#8");
            // Encrypt the key with the provided password
            Ok(encrypt_pki(&pki, &password).to_string())
        }
    }
}
