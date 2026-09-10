use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ic_agent::{
    Identity,
    export::Principal,
    identity::{
        AnonymousIdentity, BasicIdentity, DelegatedIdentity, Delegation as AgentDelegation,
        DelegationError, Prime256v1Identity, Secp256k1Identity,
        SignedDelegation as AgentSignedDelegation,
    },
};
use ic_certification::LookupResult;
use ic_ed25519::PrivateKeyFormat;
use ic_identity_hsm::HardwareIdentity;
use keyring::Entry;
use pem::Pem;
use pkcs8::{
    DecodePrivateKey, EncodePrivateKey, EncryptedPrivateKeyInfo, PrivateKeyInfo, SecretDocument,
    pkcs5::pbes2::Parameters, spki::SubjectPublicKeyInfoRef,
};
use rand::Rng;
use scrypt::Params;
use sec1::{der::Decode, pem::PemLabel};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use tracing::{debug, warn};
use url::Url;
use zeroize::Zeroizing;

use crate::{
    context::IC_ROOT_KEY,
    fs::{
        self,
        lock::{LRead, LWrite},
    },
    identity::{
        IdentityPaths, PasswordFunc,
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
        "the delegation chain loaded from `{path}` does not verify against the selected \
         network's root key; this identity was most likely issued for a different network"
    ))]
    ValidateDelegationChainNetwork {
        path: PathBuf,
        source: DelegationError,
    },

    #[snafu(display(
        "delegation for identity `{name}` has expired or will expire within 5 minutes; \
         run `icp identity reauth {name}` to re-authenticate"
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
    dirs: LWrite<&IdentityPaths>,
    list: &IdentityList,
    name: &str,
    password_func: PasswordFunc,
    network_root_key: Option<&[u8]>,
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
        IdentitySpec::WebAuth {
            algorithm, storage, ..
        } => load_webauth_identity(
            dirs.read(),
            name,
            algorithm,
            storage,
            password_func,
            network_root_key,
        ),
        IdentitySpec::PendingDelegation { .. } => DelegationNotYetProvidedSnafu { name }.fail(),
        IdentitySpec::Delegation {
            algorithm, storage, ..
        } => load_webauth_identity(
            dirs.read(),
            name,
            algorithm,
            storage,
            password_func,
            network_root_key,
        ),
    }
}

fn load_pem_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    format: &PemFormat,
    algorithm: &IdentityKeyAlgorithm,
    password_func: PasswordFunc,
    pem_session_duration: Option<Duration>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    // For password-protected PEMs, check for a valid cached session delegation first.
    if *format == PemFormat::Pbes2
        && let Some(id) = try_load_pem_session(dirs.read(), name)
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
    {
        match create_pem_session_and_build_identity(dirs, name, &*identity, duration) {
            Ok(delegated) => return Ok(delegated),
            Err(e) => debug!(identity = name, "failed to create session delegation: {e}"),
        }
    }

    Ok(identity)
}

fn load_pbes2_identity(
    doc: &Pem,
    algorithm: &IdentityKeyAlgorithm,
    password_func: PasswordFunc,
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
    let chain = delegation::load(&chain_path)
        .inspect_err(|e| debug!(identity = name, "no cached session chain: {e}"))
        .ok()?;

    if delegation::is_expiring_soon(&chain, TWO_MINUTES_NANOS)
        .inspect_err(|e| debug!(identity = name, "failed to check session expiry: {e}"))
        .ok()?
    {
        debug!(
            identity = name,
            "cached session is expiring soon; will re-authenticate"
        );
        return None;
    }

    // Load session key from keyring.
    let username = dlg_keyring_key(name);
    let entry = Entry::new(SERVICE_NAME, &username)
        .inspect_err(|e| {
            debug!(
                identity = name,
                "failed to open keyring entry for session key: {e}"
            )
        })
        .ok()?;
    let pem_str = entry
        .get_password()
        .inspect_err(|e| {
            debug!(
                identity = name,
                "failed to read session key from keyring: {e}"
            )
        })
        .ok()?;
    let origin = PemOrigin::Keyring {
        service: SERVICE_NAME.to_string(),
        username,
    };
    let pem = pem_str
        .parse::<Pem>()
        .inspect_err(|e| debug!(identity = name, "failed to parse session key PEM: {e}"))
        .ok()?;
    let session_identity =
        load_plaintext_identity(&pem, &IdentityKeyAlgorithm::Prime256v1, &origin)
            .inspect_err(|e| debug!(identity = name, "failed to load session identity: {e}"))
            .ok()?;

    let (from_key, signed_delegations) = delegation::to_agent_types(&chain)
        .inspect_err(|e| {
            debug!(
                identity = name,
                "failed to convert session delegation chain: {e}"
            )
        })
        .ok()?;
    DelegatedIdentity::new(from_key, Box::new(session_identity), signed_delegations)
        .inspect_err(|e| {
            debug!(
                identity = name,
                "failed to construct delegated identity: {e}"
            )
        })
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

    #[snafu(display("a calculated timestamp exceeds internal limits"))]
    TimeWrap,
}

/// Creates a short-lived P256 session delegation signed by `identity`, stores the session
/// key in the keyring and the chain on disk, and returns a `DelegatedIdentity`.
///
/// Returns an error if any step fails. Automatic callers silence errors via `let Ok(...) =`;
/// explicit callers propagate them.
fn create_pem_session_and_build_identity(
    dirs: LWrite<&IdentityPaths>,
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
    .ok()
    .context(TimeWrapSnafu)?;
    let expiration =
        now_nanos.saturating_add(duration.as_nanos().try_into().ok().context(TimeWrapSnafu)?);

    let agent_delegation = AgentDelegation {
        pubkey: session_pubkey.clone(),
        expiration,
        targets: None,
        permissions: None,
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
/// The `duration` should already include the 2-minute clock-drift boost.
pub fn create_explicit_pem_session(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    algorithm: &IdentityKeyAlgorithm,
    password_func: PasswordFunc,
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
    pin_fn: PasswordFunc,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let identity = HardwareIdentity::new(module, slot, key_id, &*pin_fn).context(LoadHsmSnafu)?;

    Ok(Arc::new(identity))
}

const TWO_MINUTES_NANOS: u64 = 2 * 60 * 1_000_000_000;

fn load_webauth_identity(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    algorithm: &IdentityKeyAlgorithm,
    storage: &DelegationKeyStorage,
    password_func: PasswordFunc,
    network_root_key: Option<&[u8]>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let (doc, origin) = load_webauth_session_pem(dirs, name, storage)?;

    // Load the delegation chain
    let chain_path = dirs.delegation_chain_path(name);
    let stored_chain =
        delegation::load(&chain_path).context(LoadDelegationChainSnafu { path: &chain_path })?;

    // Check expiry (2 minutes grace)
    if delegation::is_expiring_soon(&stored_chain, TWO_MINUTES_NANOS)
        .context(DelegationConversionSnafu)?
    {
        return DelegationExpiredSnafu { name }.fail();
    }

    let inner: Arc<dyn Identity> = match storage {
        DelegationKeyStorage::Keyring
        | DelegationKeyStorage::Pem {
            format: PemFormat::Plaintext,
        } => load_plaintext_identity(&doc, algorithm, &origin)?,
        DelegationKeyStorage::Pem {
            format: PemFormat::Pbes2,
        } => load_pbes2_identity(&doc, algorithm, password_func, &origin)?,
    };

    build_delegated_identity(name, &chain_path, &stored_chain, inner, network_root_key)
}

/// Assembles the delegated identity for a stored chain, verifying the chain first.
///
/// A resolved `network_root_key` is authoritative and the only key consulted: it is the key the
/// network this command talks to verifies against, so a chain failing it belongs to another
/// network.
///
/// `network_root_key` is `None` for callers that resolve no network at all, such as
/// `icp identity principal`. Mainnet is then the only key on hand, and a canister signature from
/// another provider — a local Internet Identity, say — cannot be checked: that provider's root key
/// is not derivable from anything the identity stores. Rather than make those commands unusable,
/// the links that are canister signatures are set aside and everything else about the chain is
/// verified; see [`verify_past_canister_signatures`].
fn build_delegated_identity(
    name: &str,
    chain_path: &Path,
    stored_chain: &delegation::DelegationChain,
    inner: Arc<dyn Identity>,
    network_root_key: Option<&[u8]>,
) -> Result<Arc<dyn Identity>, LoadIdentityError> {
    let (from_key, signed_delegations) =
        delegation::to_agent_types(stored_chain).context(DelegationConversionSnafu)?;

    // A resolved network root key is authoritative: it is the key the network this command talks
    // to verifies against. With no network resolved, mainnet is the only assumption available.
    let root_key = network_root_key.unwrap_or(IC_ROOT_KEY);

    match DelegatedIdentity::new_with_root_key(
        from_key.clone(),
        Box::new(Arc::clone(&inner)),
        signed_delegations.clone(),
        root_key,
    ) {
        Ok(delegated) => Ok(Arc::new(delegated)),

        // No root key to check the signature against, so verify what needs none and accept.
        Err(DelegationError::InvalidCanisterSignature(_)) if network_root_key.is_none() => {
            verify_past_canister_signatures(&from_key, &signed_delegations, &inner)
                .context(ValidateDelegationChainSnafu { path: chain_path })?;

            warn!(
                "delegation chain for identity `{name}` carries a canister signature and no root \
                 key was resolved to check it against; the rest of the chain verified, and only \
                 the network that issued it will accept it"
            );

            Ok(Arc::new(DelegatedIdentity::new_unchecked(
                from_key,
                Box::new(inner),
                signed_delegations,
            )))
        }

        Err(e @ DelegationError::InvalidCanisterSignature(_)) => {
            Err(e).context(ValidateDelegationChainNetworkSnafu { path: chain_path })
        }
        Err(e) => Err(e).context(ValidateDelegationChainSnafu { path: chain_path }),
    }
}

/// Verifies every link of a chain as far as it can be verified without a root key.
///
/// Links are classified by the type of the key that signed them, never by the error they produced:
/// ic-agent reports corruption through the same `InvalidCanisterSignature` variant as a trust-root
/// mismatch, so an error alone cannot say whether a link is unverifiable or damaged. Only a
/// leading run of canister-signed links is set aside, since ic-agent verifies a chain from its
/// root outwards and cannot resume past one further in.
fn verify_past_canister_signatures(
    from_key: &[u8],
    delegations: &[AgentSignedDelegation],
    session: &Arc<dyn Identity>,
) -> Result<(), DelegationError> {
    for (i, signed) in delegations.iter().enumerate() {
        let signer = signer_of(from_key, delegations, i);
        if is_canister_signature_key(signer) {
            verify_canister_signature_structure(signer, signed)?;
        }
    }

    let leading = (0..delegations.len())
        .take_while(|i| is_canister_signature_key(signer_of(from_key, delegations, *i)))
        .count();

    DelegatedIdentity::new_with_root_key(
        signer_of(from_key, delegations, leading).to_vec(),
        Box::new(Arc::clone(session)),
        delegations[leading..].to_vec(),
        IC_ROOT_KEY,
    )
    .map(|_| ())
}

/// The key that signed `delegations[i]`, which is the chain root for the first link.
fn signer_of<'a>(
    from_key: &'a [u8],
    delegations: &'a [AgentSignedDelegation],
    i: usize,
) -> &'a [u8] {
    match i.checked_sub(1) {
        None => from_key,
        Some(previous) => delegations[previous].delegation.pubkey.as_slice(),
    }
}

/// CBOR body of a canister signature per the IC interface spec.
///
/// The wire encoding is `tag(55799, {"certificate": bytes, "tree": hash-tree})`; `serde_cbor`
/// strips the tag transparently.
#[derive(Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
struct CanisterSignature {
    #[serde(with = "serde_bytes")]
    certificate: Vec<u8>,
    tree: ic_certification::HashTree,
}

/// Checks everything about a canister signature that does not depend on a root key.
///
/// Of the verification the IC interface spec lays out for canister signatures, only step 3 — BLS
/// verification of the certificate against the network's root key — needs a root key. This runs
/// the others: that the signature and certificate decode, that the certified data recorded for the
/// signing canister matches the signature tree, and that the tree carries a signature over exactly
/// this delegation. What remains unchecked is whether the certificate is genuine, which the
/// network settles on ingress.
///
/// ic-agent performs all of this together in `DelegatedIdentity::new_with_root_key` and exposes no
/// way to run the root-key-independent part alone, so it is repeated here. Tracked upstream as
/// dfinity/agent-rs#742; this function can go once that lands.
fn verify_canister_signature_structure(
    signing_key: &[u8],
    signed: &AgentSignedDelegation,
) -> Result<(), DelegationError> {
    let invalid = |message: String| {
        DelegationError::InvalidCanisterSignature(format!(
            "{message} (the certificate's own signature is not covered by this check)"
        ))
    };

    let (canister_id, seed) = parse_canister_signature_key(signing_key)
        .ok_or_else(|| invalid("malformed canister signature public key".into()))?;

    let signature: CanisterSignature = serde_cbor::from_slice(&signed.signature)
        .map_err(|e| invalid(format!("invalid canister signature CBOR: {e}")))?;
    let certificate: ic_certification::Certificate = serde_cbor::from_slice(&signature.certificate)
        .map_err(|e| invalid(format!("invalid certificate CBOR: {e}")))?;

    let certified_data_path: [&[u8]; 3] = [b"canister", canister_id.as_slice(), b"certified_data"];
    let certified_data = match certificate.tree.lookup_path(certified_data_path) {
        LookupResult::Found(value) => value,
        _ => {
            return Err(invalid(
                "certified_data is absent from the certificate".into(),
            ));
        }
    };
    if certified_data != signature.tree.digest().as_ref() {
        return Err(invalid(
            "certified_data does not match the signature tree".into(),
        ));
    }

    let seed_hash: [u8; 32] = Sha256::digest(&seed).into();
    let payload_hash: [u8; 32] = Sha256::digest(signed.delegation.signable()).into();
    match signature
        .tree
        .lookup_path([&b"sig"[..], &seed_hash, &payload_hash])
    {
        LookupResult::Found([]) => Ok(()),
        _ => Err(invalid(
            "the signature tree carries no signature over this delegation".into(),
        )),
    }
}

/// Splits a canister-signature public key into the signing canister and its seed.
///
/// The key's BIT STRING is `canister_id_length | canister_id | seed` per the IC interface spec.
fn parse_canister_signature_key(der: &[u8]) -> Option<(Principal, Vec<u8>)> {
    let spki = SubjectPublicKeyInfoRef::from_der(der).ok()?;
    let raw = spki.subject_public_key.raw_bytes();

    let (&length, rest) = raw.split_first()?;
    let (canister_id, seed) = rest.split_at_checked(length as usize)?;

    Some((Principal::try_from_slice(canister_id).ok()?, seed.to_vec()))
}

/// Reports whether `der` is a canister-signature public key (OID 1.3.6.1.4.1.56387.1.2).
///
/// Signatures under such a key are IC certificates, verifiable only against the root key of the
/// network whose canister produced them.
fn is_canister_signature_key(der: &[u8]) -> bool {
    const CANISTER_SIG_OID: pkcs8::ObjectIdentifier =
        pkcs8::ObjectIdentifier::new_unwrap("1.3.6.1.4.1.56387.1.2");

    SubjectPublicKeyInfoRef::from_der(der).is_ok_and(|spki| spki.algorithm.oid == CANISTER_SIG_OID)
}

/// Returns the DER-encoded public key for a stored web-auth session key.
///
/// Used during re-authentication to obtain the session public key without
/// re-loading the full delegated identity.
pub fn load_webauth_session_public_key(
    dirs: LRead<&IdentityPaths>,
    name: &str,
    algorithm: &IdentityKeyAlgorithm,
    storage: &DelegationKeyStorage,
    password_func: PasswordFunc,
) -> Result<Vec<u8>, LoadIdentityError> {
    let (doc, origin) = load_webauth_session_pem(dirs, name, storage)?;

    match storage {
        DelegationKeyStorage::Keyring
        | DelegationKeyStorage::Pem {
            format: PemFormat::Plaintext,
        } => load_webauth_public_key_plaintext(&doc, algorithm, &origin),
        DelegationKeyStorage::Pem {
            format: PemFormat::Pbes2,
        } => {
            let pw = password_func()
                .map_err(|message| LoadIdentityError::GetPasswordError { message })?;
            load_webauth_public_key_pbes2(&doc, algorithm, &origin, &pw)
        }
    }
}

fn load_webauth_session_pem(
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

fn load_webauth_public_key_plaintext(
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

fn load_webauth_public_key_pbes2(
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
    dirs: LWrite<&IdentityPaths>,
    password_func: PasswordFunc,
    pem_session_duration: Option<Duration>,
) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
    let identity = load_identity(
        dirs,
        &IdentityList::load_from(dirs.read())?,
        &(IdentityDefaults::load_from(dirs.read())?).default,
        password_func,
        None,
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

    #[snafu(display("delegation chain contains no delegations"))]
    CreateIdentityEmptyDelegation,

    #[snafu(display("invalid session public key in delegation chain"))]
    CreateIdentityDecodeLeafKey { source: hex::FromHexError },

    #[snafu(display(
        "the imported key does not match the session key the delegation chain was issued to"
    ))]
    CreateIdentityKeyMismatch,

    #[snafu(transparent)]
    CreateIdentityValidateDelegationChain {
        source: ValidateDelegationChainError,
    },

    #[snafu(display(
        "delegation chain has already expired (or is about to); import a freshly signed chain"
    ))]
    CreateIdentityDelegationExpired,

    #[snafu(display("failed to create delegation directory"))]
    CreateIdentityDelegationDir { source: crate::fs::IoError },

    #[snafu(display("failed to save delegation chain to `{path}`"))]
    CreateIdentitySaveDelegation {
        path: PathBuf,
        source: delegation::SaveError,
    },
}

/// Creates a new identity from `key`, stored according to `format`.
///
/// If `delegation` is supplied, the identity is registered as a delegation-based identity
/// (as if created via `icp identity delegation use`): `key` is stored as the chain's
/// session key, the chain is saved to disk, and the identity's principal is derived from
/// the chain's root key. `key` must be the session key the chain's leaf delegation was
/// issued to, which is verified before anything is written.
pub fn create_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    key: IdentityKey,
    format: CreateFormat,
    delegation: Option<&delegation::DelegationChain>,
) -> Result<(), CreateIdentityError> {
    let mut identity_list = IdentityList::load_from(dirs.read())?;
    ensure!(
        !identity_list.identities.contains_key(name),
        IdentityAlreadyExistsSnafu { name }
    );
    let algorithm = match &key {
        IdentityKey::Secp256k1(_) => IdentityKeyAlgorithm::Secp256k1,
        IdentityKey::Prime256v1(_) => IdentityKeyAlgorithm::Prime256v1,
        IdentityKey::Ed25519(_) => IdentityKeyAlgorithm::Ed25519,
    };

    // For a plain identity the principal is the key's own; for a delegation identity it
    // comes from the chain's root key, and the imported key is verified to be the session
    // key the chain delegates to (catching the wrong key here, not as a load-time failure).
    let principal = if let Some(chain) = delegation {
        // The imported key must be the session key the chain's leaf delegation was issued to.
        // Check that explicitly first, for a clearer error than the full validation below gives.
        let session = session_identity_for_validation(&key);
        let session_public_key = session
            .public_key()
            .expect("non-anonymous identity always has a public key");
        let leaf = chain
            .delegations
            .last()
            .context(CreateIdentityEmptyDelegationSnafu)?;
        let leaf_public_key =
            hex::decode(&leaf.delegation.pubkey).context(CreateIdentityDecodeLeafKeySnafu)?;
        ensure!(
            leaf_public_key == session_public_key,
            CreateIdentityKeyMismatchSnafu
        );

        // Validate the whole chain in memory before persisting anything, so a structurally
        // broken chain fails here rather than on every later load.
        let from_key = validate_session_delegation_chain(name, &session, chain)?;

        // Reject a chain that has already expired (or falls within the load-time grace
        // window): it would import successfully but then fail on every later load with
        // `DelegationExpired`. Mirrors the expiry check in `load_webauth_identity`.
        if delegation::is_expiring_soon(chain, TWO_MINUTES_NANOS).context(ConvertChainSnafu)? {
            return CreateIdentityDelegationExpiredSnafu.fail();
        }

        ic_agent::export::Principal::self_authenticating(&from_key)
    } else {
        match &key {
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
        }
    };

    let doc = match key {
        IdentityKey::Secp256k1(key) => key.to_pkcs8_der().expect("infallible PKI encoding"),
        IdentityKey::Prime256v1(key) => key.to_pkcs8_der().expect("infallible PKI encoding"),
        IdentityKey::Ed25519(key) => key
            .serialize_pkcs8(PrivateKeyFormat::Pkcs8v2)
            .try_into()
            .expect("infallible PKI encoding"),
    };
    // store key material. Delegation session keys stored in the keyring use the
    // `delegation:` prefix so `load_webauth_session_pem` can find them.
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
            let username = if delegation.is_some() {
                dlg_keyring_key(name)
            } else {
                name.to_string()
            };
            let entry = Entry::new(SERVICE_NAME, &username).context(CreateEntrySnafu)?;
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

    // Whether a session key was just stored in the keyring that must be rolled back if the
    // writes below fail: the manifest would never be written, so `identity remove` could not
    // later find the orphaned `delegation:<name>` credential.
    let rollback_keyring = delegation.is_some() && matches!(format, CreateFormat::Keyring);

    let spec = if delegation.is_some() {
        let storage = match format {
            CreateFormat::Plaintext => DelegationKeyStorage::Pem {
                format: PemFormat::Plaintext,
            },
            CreateFormat::Pbes2 { .. } => DelegationKeyStorage::Pem {
                format: PemFormat::Pbes2,
            },
            CreateFormat::Keyring => DelegationKeyStorage::Keyring,
        };
        IdentitySpec::Delegation {
            algorithm,
            principal,
            storage,
        }
    } else {
        match format {
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
        }
    };

    let persist = || -> Result<(), CreateIdentityError> {
        if let Some(chain) = delegation {
            let delegation_path = dirs
                .ensure_delegation_chain_path(name)
                .context(CreateIdentityDelegationDirSnafu)?;
            delegation::save(&delegation_path, chain).context(
                CreateIdentitySaveDelegationSnafu {
                    path: &delegation_path,
                },
            )?;
        }
        identity_list.identities.insert(name.to_string(), spec);
        identity_list.write_to(dirs)?;
        Ok(())
    };
    if let Err(e) = persist() {
        if rollback_keyring && let Ok(entry) = Entry::new(SERVICE_NAME, &dlg_keyring_key(name)) {
            let _ = entry.delete_credential();
        }
        return Err(e);
    }

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
        WebAuthKeyringAndDelegation(Entry, PathBuf),
        WebAuthPemAndDelegation(PathBuf, PathBuf),
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
        IdentitySpec::WebAuth { storage, .. } => {
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
                    OldKeyMaterial::WebAuthKeyringAndDelegation(old_entry, old_delegation)
                }
                DelegationKeyStorage::Pem { .. } => {
                    let old_pem = dirs.key_pem_path(old_name);
                    let new_pem = dirs.key_pem_path(new_name);
                    let contents = fs::read(&old_pem).context(CopyKeyFileSnafu)?;
                    fs::write(&new_pem, &contents).context(CopyKeyFileSnafu)?;
                    OldKeyMaterial::WebAuthPemAndDelegation(old_pem, old_delegation)
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
                    OldKeyMaterial::WebAuthKeyringAndDelegation(old_entry, old_delegation)
                }
                DelegationKeyStorage::Pem { .. } => {
                    let old_pem = dirs.key_pem_path(old_name);
                    let new_pem = dirs.key_pem_path(new_name);
                    let contents = fs::read(&old_pem).context(CopyKeyFileSnafu)?;
                    fs::write(&new_pem, &contents).context(CopyKeyFileSnafu)?;
                    OldKeyMaterial::WebAuthPemAndDelegation(old_pem, old_delegation)
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
        OldKeyMaterial::WebAuthKeyringAndDelegation(old_entry, old_delegation) => {
            old_entry
                .delete_credential()
                .context(DeleteKeyringEntrySnafu { old_name })?;
            fs::remove_file(&old_delegation).context(DeleteOldKeyFileSnafu)?;
        }
        OldKeyMaterial::WebAuthPemAndDelegation(old_pem, old_delegation) => {
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
        IdentitySpec::WebAuth { storage, .. } => {
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

    #[snafu(transparent)]
    DlgValidateDelegationChain {
        source: ValidateDelegationChainError,
    },

    #[snafu(display("malformed delegation chain"))]
    DlgConvertChain { source: delegation::ConversionError },

    #[snafu(display(
        "delegation chain has already expired (or is about to); log in again to get a fresh one"
    ))]
    DlgDelegationExpired,
}

/// Constructs a temporary signing identity directly from an [`IdentityKey`], used to validate a
/// delegation chain before storing it.
fn session_identity_for_validation(key: &IdentityKey) -> Arc<dyn Identity> {
    match key {
        IdentityKey::Ed25519(k) => Arc::new(BasicIdentity::from_raw_key(&k.serialize_raw())),
        IdentityKey::Secp256k1(k) => Arc::new(Secp256k1Identity::from_private_key(k.clone())),
        IdentityKey::Prime256v1(k) => Arc::new(Prime256v1Identity::from_private_key(k.clone())),
    }
}

#[derive(Debug, Snafu)]
pub enum ValidateDelegationChainError {
    #[snafu(display("malformed delegation chain"))]
    ConvertChain { source: delegation::ConversionError },

    #[snafu(display("delegation chain failed validation"))]
    ValidateChain { source: DelegationError },
}

/// Validates that `chain` connects its root key to `session`'s public key and returns the
/// DER-encoded chain root (`from_key`), from which the identity's principal is derived.
///
/// The chain is verified against the IC mainnet root key. A canister signature it cannot verify is
/// downgraded to a warning — the chain most likely targets a non-mainnet network, and no root key
/// is available here to confirm that — but the chain must still hand authority to `session`. Any
/// other validation failure is an error. `session` is the temporary signing identity built from
/// the session key (see [`session_identity_for_validation`]).
fn validate_session_delegation_chain(
    name: &str,
    session: &Arc<dyn Identity>,
    chain: &delegation::DelegationChain,
) -> Result<Vec<u8>, ValidateDelegationChainError> {
    let (from_key, delegations) = delegation::to_agent_types(chain).context(ConvertChainSnafu)?;

    match DelegatedIdentity::new(
        from_key.clone(),
        Box::new(Arc::clone(session)),
        delegations.clone(),
    ) {
        Ok(_) => return Ok(from_key),
        // Nothing here resolves a network, so a canister signature from a non-mainnet provider
        // cannot be checked. Fall through to the checks that need no root key.
        Err(DelegationError::InvalidCanisterSignature(_)) => {}
        Err(e) => return Err(e).context(ValidateChainSnafu),
    }

    // `DelegatedIdentity::new` stopped at the canister-signed link, leaving the rest of the chain
    // unexamined. Nothing here resolves a network, so verify everything the root key does not
    // decide — including that the chain was issued to this session key.
    verify_past_canister_signatures(&from_key, &delegations, session)
        .context(ValidateChainSnafu)?;

    warn!(
        "delegation chain for identity `{name}` carries a canister signature that the IC mainnet \
         root key does not verify; this identity is only usable on the network that issued it"
    );

    Ok(from_key)
}

/// Links a web-auth identity to a new named identity.
///
/// Stores the session keypair according to `storage` and the delegation chain
/// as a separate JSON file.
pub fn link_webauth_identity(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    key: IdentityKey,
    chain: &delegation::DelegationChain,
    principal: ic_agent::export::Principal,
    create_format: CreateFormat,
    host: Url,
    domain: Option<String>,
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

    // Validate the delegation chain against the mainnet root key before storing it.
    let session = session_identity_for_validation(&key);
    validate_session_delegation_chain(name, &session, chain)?;

    // Reject a chain that has already expired (or falls within the load-time grace window): it
    // would link successfully but then fail on every later load. Mirrors the checks in
    // `create_identity` and `load_webauth_identity`.
    ensure!(
        !delegation::is_expiring_soon(chain, TWO_MINUTES_NANOS).context(DlgConvertChainSnafu)?,
        DlgDelegationExpiredSnafu
    );

    let doc = match key {
        IdentityKey::Secp256k1(key) => key.to_pkcs8_der().expect("infallible PKI encoding"),
        IdentityKey::Prime256v1(key) => key.to_pkcs8_der().expect("infallible PKI encoding"),
        IdentityKey::Ed25519(key) => key
            .serialize_pkcs8(PrivateKeyFormat::Pkcs8v2)
            .try_into()
            .expect("infallible PKI encoding"),
    };

    let webauth_storage = match &create_format {
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

    let spec = IdentitySpec::WebAuth {
        algorithm,
        principal,
        storage: webauth_storage,
        host,
        domain,
    };
    identity_list.identities.insert(name.to_string(), spec);
    identity_list.write_to(dirs)?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum UpdateWebAuthDelegationError {
    #[snafu(transparent)]
    LoadIdentityManifest { source: LoadIdentityManifestError },

    #[snafu(display("no identity found with name `{name}`"))]
    WebAuthIdentityNotFound { name: String },

    #[snafu(display("identity `{name}` is not web-based"))]
    NotWebBased { name: String },

    #[snafu(display("failed to save delegation chain to `{path}`"))]
    UpdateWebAuthDelegationSave {
        path: PathBuf,
        source: delegation::SaveError,
    },

    #[snafu(display("failed to create delegation directory"))]
    UpdateWebAuthCreateDir { source: crate::fs::IoError },
}

/// Updates the delegation chain for an existing web-based identity.
pub fn update_webauth_delegation(
    dirs: LWrite<&IdentityPaths>,
    name: &str,
    chain: &delegation::DelegationChain,
) -> Result<(), UpdateWebAuthDelegationError> {
    let identity_list = IdentityList::load_from(dirs.read())?;
    let spec = identity_list
        .identities
        .get(name)
        .context(WebAuthIdentityNotFoundSnafu { name })?;

    ensure!(
        matches!(spec, IdentitySpec::WebAuth { .. }),
        NotWebBasedSnafu { name }
    );

    let delegation_path = dirs
        .ensure_delegation_chain_path(name)
        .context(UpdateWebAuthCreateDirSnafu)?;
    delegation::save(&delegation_path, chain).context(UpdateWebAuthDelegationSaveSnafu {
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

    #[snafu(display("cannot export a delegation-based identity"))]
    CannotExportDelegationBased,

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
        IdentitySpec::WebAuth { .. } => return CannotExportDelegationBasedSnafu.fail(),
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

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_FROM_NOW_NANOS: u64 = 3600 * 1_000_000_000;

    /// The root key of some other network — here, one that verifies nothing.
    const OTHER_NETWORK_ROOT_KEY: &[u8] = &[0u8; 133];

    fn new_session() -> (Arc<dyn Identity>, Vec<u8>) {
        let key = ic_ed25519::PrivateKey::generate();
        let identity = BasicIdentity::from_raw_key(&key.serialize_raw());
        let public_key = identity
            .public_key()
            .expect("ed25519 always has a public key");
        (Arc::new(identity), public_key)
    }

    fn new_signer() -> BasicIdentity {
        BasicIdentity::from_raw_key(&ic_ed25519::PrivateKey::generate().serialize_raw())
    }

    fn now_plus(offset: u64) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos() as u64
            + offset
    }

    /// A link handing authority to `to`, signed by `signer`.
    fn signed_link(signer: &dyn Identity, to: &[u8]) -> delegation::SignedDelegation {
        signed_link_expiring_at(signer, to, now_plus(HOUR_FROM_NOW_NANOS))
    }

    fn signed_link_expiring_at(
        signer: &dyn Identity,
        to: &[u8],
        expiration: u64,
    ) -> delegation::SignedDelegation {
        let delegation = AgentDelegation {
            pubkey: to.to_vec(),
            expiration,
            targets: None,
            permissions: None,
        };
        let signature = signer
            .sign_delegation(&delegation)
            .expect("signing a delegation should succeed")
            .signature
            .expect("a signed delegation carries a signature");

        delegation::SignedDelegation {
            signature: hex::encode(signature),
            delegation: delegation::Delegation {
                pubkey: hex::encode(to),
                expiration: format!("{expiration:x}"),
                targets: None,
            },
        }
    }

    /// A DER canister-signature public key (OID 1.3.6.1.4.1.56387.1.2).
    ///
    /// Nothing available to a unit test can verify a signature under such a key: doing so means
    /// BLS-verifying an IC certificate against the root key of the network whose canister produced
    /// it. That is exactly the position `icp identity principal` is in.
    fn canister_sig_public_key() -> Vec<u8> {
        const OID: [u8; 10] = [0x2b, 0x06, 0x01, 0x04, 0x01, 0x83, 0xb8, 0x43, 0x01, 0x02];
        let canister_id = [0x0a, 0, 0, 0, 0, 0, 0, 0, 0x07, 0x01, 0x01];

        let mut raw = vec![canister_id.len() as u8];
        raw.extend_from_slice(&canister_id);
        raw.extend_from_slice(b"seed");

        let mut algorithm = vec![0x30, (OID.len() + 2) as u8, 0x06, OID.len() as u8];
        algorithm.extend_from_slice(&OID);

        let mut bit_string = vec![0x03, (raw.len() + 1) as u8, 0x00];
        bit_string.extend_from_slice(&raw);

        let mut spki = vec![0x30, (algorithm.len() + bit_string.len()) as u8];
        spki.extend_from_slice(&algorithm);
        spki.extend_from_slice(&bit_string);
        spki
    }

    /// A link under a canister-signature key, as a local Internet Identity issues.
    ///
    /// The signature is structurally sound — its CBOR parses, the certificate records the
    /// signature tree as the signing canister's certified data, and the tree carries a signature
    /// over exactly this delegation — but its certificate is signed by nothing. Only a root key
    /// could tell it apart from a genuine one, which is the position a caller with no network is
    /// in.
    fn canister_sig_link(to: &[u8]) -> delegation::SignedDelegation {
        let expiration = now_plus(HOUR_FROM_NOW_NANOS);
        let delegation = AgentDelegation {
            pubkey: to.to_vec(),
            expiration,
            targets: None,
            permissions: None,
        };

        let (canister_id, seed) =
            parse_canister_signature_key(&canister_sig_public_key()).expect("well-formed key");
        let seed_hash: [u8; 32] = Sha256::digest(&seed).into();
        let payload_hash: [u8; 32] = Sha256::digest(delegation.signable()).into();

        let sig_tree = ic_certification::labeled(
            &b"sig"[..],
            ic_certification::labeled(
                &seed_hash[..],
                ic_certification::labeled(&payload_hash[..], ic_certification::leaf(vec![])),
            ),
        );
        let certificate = ic_certification::Certificate {
            tree: ic_certification::labeled(
                &b"canister"[..],
                ic_certification::labeled(
                    canister_id.as_slice(),
                    ic_certification::labeled(
                        &b"certified_data"[..],
                        ic_certification::leaf(sig_tree.digest().to_vec()),
                    ),
                ),
            ),
            signature: vec![0; 48],
            delegation: None,
        };

        let signature = encode_canister_signature(CanisterSignature {
            certificate: serde_cbor::to_vec(&certificate).expect("certificate encodes"),
            tree: sig_tree,
        });

        delegation::SignedDelegation {
            signature: hex::encode(signature),
            delegation: delegation::Delegation {
                pubkey: hex::encode(to),
                expiration: format!("{expiration:x}"),
                targets: None,
            },
        }
    }

    /// Encodes a canister signature the way the wire carries one: wrapped in the self-describing
    /// CBOR tag 55799, as every signature a real auth provider issues is.
    fn encode_canister_signature(signature: CanisterSignature) -> Vec<u8> {
        use serde::Serialize;

        let mut encoded = Vec::new();
        let mut serializer =
            serde_cbor::Serializer::new(serde_cbor::ser::IoWrite::new(&mut encoded));
        serializer.self_describe().expect("tag writes");
        signature
            .serialize(&mut serializer)
            .expect("signature encodes");

        assert_eq!(
            &encoded[..3],
            &[0xd9, 0xd9, 0xf7],
            "the fixture must carry the tag a real signature does"
        );
        encoded
    }

    /// The same shape, but with the signature bytes replaced by rubbish.
    fn corrupt_canister_sig_link(to: &[u8]) -> delegation::SignedDelegation {
        let mut link = canister_sig_link(to);
        link.signature = hex::encode([0xde, 0xad, 0xbe, 0xef]);
        link
    }

    fn chain_of(
        public_key: &[u8],
        delegations: Vec<delegation::SignedDelegation>,
    ) -> delegation::DelegationChain {
        delegation::DelegationChain {
            public_key: hex::encode(public_key),
            delegations,
        }
    }

    fn tampered(mut link: delegation::SignedDelegation) -> delegation::SignedDelegation {
        link.signature = hex::encode([0u8; 64]);
        link
    }

    /// The two-link shape a real Internet Identity issues: a canister signature to a browser
    /// session key, then an ordinary signature to the key the CLI holds.
    fn ii_shaped_chain(
        session_key: &[u8],
        tamper_second_link: bool,
    ) -> delegation::DelegationChain {
        let intermediate = new_signer();
        let intermediate_key = intermediate.public_key().expect("public key");
        let second = signed_link(&intermediate, session_key);

        chain_of(
            &canister_sig_public_key(),
            vec![
                canister_sig_link(&intermediate_key),
                if tamper_second_link {
                    tampered(second)
                } else {
                    second
                },
            ],
        )
    }

    fn load(
        chain: &delegation::DelegationChain,
        session: Arc<dyn Identity>,
        network_root_key: Option<&[u8]>,
    ) -> Result<Arc<dyn Identity>, LoadIdentityError> {
        build_delegated_identity(
            "test",
            Path::new("chain.json"),
            chain,
            session,
            network_root_key,
        )
    }

    #[test]
    fn canister_signature_keys_are_recognised_by_their_oid() {
        let (_, ed25519_key) = new_session();
        assert!(is_canister_signature_key(&canister_sig_public_key()));
        assert!(!is_canister_signature_key(&ed25519_key));
        assert!(!is_canister_signature_key(b"not a key"));
    }

    #[test]
    fn unverifiable_canister_signature_is_accepted_when_no_root_key_is_resolved() {
        let (session, session_key) = new_session();
        let chain = chain_of(
            &canister_sig_public_key(),
            vec![canister_sig_link(&session_key)],
        );

        let identity = load(&chain, session, None).expect("accepted without a root key");

        assert_eq!(
            identity.sender().expect("sender"),
            Principal::self_authenticating(canister_sig_public_key()),
        );
    }

    #[test]
    fn unverifiable_canister_signature_is_rejected_when_issued_to_another_session() {
        let (session, _) = new_session();
        let (_, other_key) = new_session();
        let chain = chain_of(
            &canister_sig_public_key(),
            vec![canister_sig_link(&other_key)],
        );

        assert!(matches!(
            load(&chain, session, None),
            Err(LoadIdentityError::ValidateDelegationChain { .. })
        ));
    }

    #[test]
    fn links_behind_a_canister_signature_are_verified_when_no_root_key_is_resolved() {
        let (session, session_key) = new_session();

        load(
            &ii_shaped_chain(&session_key, false),
            Arc::clone(&session),
            None,
        )
        .expect("a sound chain behind the canister signature is accepted");

        assert!(
            matches!(
                load(&ii_shaped_chain(&session_key, true), session, None),
                Err(LoadIdentityError::ValidateDelegationChain { .. })
            ),
            "a tampered link behind the canister signature must stay fatal"
        );
    }

    #[test]
    fn a_resolved_network_root_key_is_authoritative() {
        let (session, session_key) = new_session();
        let chain = chain_of(
            &canister_sig_public_key(),
            vec![canister_sig_link(&session_key)],
        );

        // The same chain that loads unverified with no root key is rejected once a root key it
        // does not verify against is on the table.
        assert!(matches!(
            load(&chain, session, Some(OTHER_NETWORK_ROOT_KEY)),
            Err(LoadIdentityError::ValidateDelegationChainNetwork { .. })
        ));
    }

    /// The root key decides canister signatures and nothing else, so a broken ordinary link must
    /// not be reported as a network mismatch.
    #[test]
    fn a_broken_link_is_not_reported_as_a_network_mismatch() {
        let (session, session_key) = new_session();
        let signer = new_signer();
        let chain = chain_of(
            &signer.public_key().expect("public key"),
            vec![tampered(signed_link(&signer, &session_key))],
        );

        assert!(matches!(
            load(&chain, session, Some(OTHER_NETWORK_ROOT_KEY)),
            Err(LoadIdentityError::ValidateDelegationChain { .. })
        ));
    }

    /// A resolved root key must not over-reject: a chain with no canister signature carries
    /// nothing that depends on a trust root, and verifies under any root key.
    #[test]
    fn a_chain_without_a_canister_signature_verifies_under_any_root_key() {
        let (session, session_key) = new_session();
        let signer = new_signer();
        let chain = chain_of(
            &signer.public_key().expect("public key"),
            vec![signed_link(&signer, &session_key)],
        );

        load(&chain, session, Some(OTHER_NETWORK_ROOT_KEY))
            .expect("a chain with no canister signature needs no particular root key");
    }

    #[test]
    fn validate_session_delegation_chain_accepts_a_mainnet_chain() {
        let key = IdentityKey::Ed25519(ic_ed25519::PrivateKey::generate());
        let session = session_identity_for_validation(&key);
        let session_key = session.public_key().expect("public key");
        let root = new_signer();
        let root_key = root.public_key().expect("public key");

        let from_key = validate_session_delegation_chain(
            "test",
            &session,
            &chain_of(&root_key, vec![signed_link(&root, &session_key)]),
        )
        .expect("a chain signed by its own root validates");

        assert_eq!(from_key, root_key);
    }

    #[test]
    fn validate_session_delegation_chain_accepts_an_unverifiable_canister_signature() {
        let key = IdentityKey::Ed25519(ic_ed25519::PrivateKey::generate());
        let session = session_identity_for_validation(&key);
        let session_key = session.public_key().expect("public key");

        validate_session_delegation_chain("test", &session, &ii_shaped_chain(&session_key, false))
            .expect("a local-provider chain links successfully");
    }

    #[test]
    fn validate_session_delegation_chain_rejects_a_broken_link_behind_a_canister_signature() {
        let key = IdentityKey::Ed25519(ic_ed25519::PrivateKey::generate());
        let session = session_identity_for_validation(&key);
        let session_key = session.public_key().expect("public key");

        assert!(
            validate_session_delegation_chain(
                "test",
                &session,
                &ii_shaped_chain(&session_key, true)
            )
            .is_err()
        );
    }

    #[test]
    fn validate_session_delegation_chain_rejects_a_chain_issued_to_another_key() {
        let key = IdentityKey::Ed25519(ic_ed25519::PrivateKey::generate());
        let session = session_identity_for_validation(&key);
        let (_, other_key) = new_session();
        let root = new_signer();

        assert!(
            validate_session_delegation_chain(
                "test",
                &session,
                &chain_of(
                    &root.public_key().expect("public key"),
                    vec![signed_link(&root, &other_key)]
                )
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn link_webauth_identity_rejects_an_expired_chain() {
        let key = ic_ed25519::PrivateKey::generate();
        let identity_key = IdentityKey::Ed25519(key);
        let session = session_identity_for_validation(&identity_key);
        let session_key = session.public_key().expect("public key");

        let root = new_signer();
        let chain = chain_of(
            &root.public_key().expect("public key"),
            vec![signed_link_expiring_at(&root, &session_key, 1)],
        );

        let tmp = camino_tempfile::tempdir().expect("tempdir");
        let dirs = IdentityPaths::new(tmp.path().to_path_buf()).expect("identity paths");
        let (result, chain_path) = dirs
            .with_write(async |dirs| {
                let chain_path = dirs.read().delegation_chain_path("expired");
                let result = link_webauth_identity(
                    dirs,
                    "expired",
                    identity_key,
                    &chain,
                    Principal::self_authenticating(root.public_key().expect("public key")),
                    CreateFormat::Plaintext,
                    Url::parse("https://id.ai").expect("url"),
                    None,
                );
                (result, chain_path)
            })
            .await
            .expect("lock");

        assert!(matches!(
            result,
            Err(CreatePendingDelegationError::DlgDelegationExpired)
        ));
        assert!(
            !chain_path.exists(),
            "an expired chain must not be persisted"
        );
    }

    /// A canister signature cannot be trusted without a root key, but it can still be checked for
    /// self-consistency, and that check must not be skipped along with the trust check.
    #[test]
    fn a_corrupt_canister_signature_is_rejected_even_with_no_root_key() {
        let (session, session_key) = new_session();
        let chain = chain_of(
            &canister_sig_public_key(),
            vec![corrupt_canister_sig_link(&session_key)],
        );

        assert!(matches!(
            load(&chain, session, None),
            Err(LoadIdentityError::ValidateDelegationChain { .. })
        ));
    }

    #[test]
    fn a_canister_signature_over_another_delegation_is_rejected() {
        let (session, session_key) = new_session();
        let (_, other_key) = new_session();

        // A signature genuinely issued, but for a different delegation than the one it is attached
        // to: the signature tree carries no entry for this payload.
        let mut link = canister_sig_link(&other_key);
        link.delegation.pubkey = hex::encode(&session_key);
        let chain = chain_of(&canister_sig_public_key(), vec![link]);

        assert!(matches!(
            load(&chain, session, None),
            Err(LoadIdentityError::ValidateDelegationChain { .. })
        ));
    }

    /// Real canister signatures arrive wrapped in the self-describing CBOR tag 55799. Decoding
    /// must see through it, and every other fixture here relies on that.
    #[test]
    fn a_tagged_canister_signature_decodes() {
        let (_, session_key) = new_session();
        let link = canister_sig_link(&session_key);
        let tagged = hex::decode(&link.signature).expect("hex");
        assert_eq!(&tagged[..3], &[0xd9, 0xd9, 0xf7], "fixture carries the tag");

        let check = |link: delegation::SignedDelegation| {
            let chain = chain_of(&canister_sig_public_key(), vec![link]);
            let (from_key, delegations) =
                delegation::to_agent_types(&chain).expect("chain converts");
            verify_canister_signature_structure(&from_key, &delegations[0])
        };

        check(link).expect("a tagged signature decodes");
    }

    /// A key is only a canister-signature key if the whole slice is that key. A valid prefix with
    /// bytes after it is malformed, and must not be set aside as unverifiable on a partial read.
    #[test]
    fn a_key_with_trailing_bytes_is_not_a_canister_signature_key() {
        let mut trailing = canister_sig_public_key();
        trailing.push(0);

        assert!(is_canister_signature_key(&canister_sig_public_key()));
        assert!(!is_canister_signature_key(&trailing));
        assert!(parse_canister_signature_key(&trailing).is_none());

        // And such a chain is rejected rather than accepted with a warning.
        let (session, session_key) = new_session();
        let chain = chain_of(&trailing, vec![canister_sig_link(&session_key)]);
        assert!(matches!(
            load(&chain, session, None),
            Err(LoadIdentityError::ValidateDelegationChain { .. })
        ));
    }

    /// Stands in for the session key a chain was issued to. Verification only asks the session
    /// identity for its principal, so a real chain can be checked without committing its key.
    struct SessionStub(Principal);

    impl Identity for SessionStub {
        fn sender(&self) -> Result<Principal, String> {
            Ok(self.0)
        }

        fn public_key(&self) -> Option<Vec<u8>> {
            None
        }

        fn sign(
            &self,
            _: &ic_agent::agent::EnvelopeContent,
        ) -> Result<ic_agent::Signature, String> {
            unreachable!("verification never signs")
        }
    }

    /// A chain a local Internet Identity actually issued, with the root key of the replica that
    /// issued it.
    ///
    /// Every other canister-signature fixture here is encoded by the same types the production
    /// code decodes with, so it is self-consistent by construction: a change that shifts encoding
    /// and decoding together — a new `ic-certification` hash-tree layout, say — would keep those
    /// tests green while rejecting every real signature. Only captured bytes catch that.
    #[test]
    fn a_real_local_ii_chain_verifies_against_the_network_that_issued_it() {
        let chain: delegation::DelegationChain =
            serde_json::from_str(include_str!("testdata/local_ii_chain.json"))
                .expect("fixture parses");
        let local_root_key = hex::decode(include_str!("testdata/local_ii_root_key.hex").trim())
            .expect("fixture root key is hex");

        let (_, delegations) = delegation::to_agent_types(&chain).expect("chain converts");
        let session_key = &delegations.last().expect("a link").delegation.pubkey;
        let session: Arc<dyn Identity> =
            Arc::new(SessionStub(Principal::self_authenticating(session_key)));

        load(&chain, Arc::clone(&session), Some(&local_root_key))
            .expect("verifies against the root key of the network that issued it");

        // With no network resolved, the canister signature cannot be trusted, but everything else
        // about the chain still checks out.
        load(&chain, Arc::clone(&session), None)
            .expect("accepted unverified when no root key is available");

        assert!(
            matches!(
                load(&chain, session, Some(IC_ROOT_KEY)),
                Err(LoadIdentityError::ValidateDelegationChainNetwork { .. })
            ),
            "mainnet's root key must reject a chain issued by a local replica"
        );
    }
}
