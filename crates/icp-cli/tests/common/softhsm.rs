use std::{env, fs, sync::OnceLock};

use cryptoki::{
    context::{CInitializeArgs, CInitializeFlags, Pkcs11},
    mechanism::Mechanism,
    object::Attribute,
    session::UserType,
    types::AuthPin,
};
use icp::prelude::*;

/// Default SoftHSM2 library paths by platform
#[cfg(target_os = "macos")]
const DEFAULT_SOFTHSM_PATHS: &[&str] = &[
    // Homebrew on Apple Silicon
    "/opt/homebrew/lib/softhsm/libsofthsm2.so",
    // Homebrew on Intel
    "/usr/local/lib/softhsm/libsofthsm2.so",
];

#[cfg(target_os = "linux")]
const DEFAULT_SOFTHSM_PATHS: &[&str] = &[
    "/usr/lib/softhsm/libsofthsm2.so",
    "/usr/lib/x86_64-linux-gnu/softhsm/libsofthsm2.so",
    "/usr/lib64/pkcs11/libsofthsm2.so",
];

#[cfg(target_os = "windows")]
const DEFAULT_SOFTHSM_PATHS: &[&str] = &[
    "C:\\SoftHSM2\\lib\\softhsm2.dll",
    "C:\\Program Files\\SoftHSM2\\lib\\softhsm2.dll",
    "C:\\Program Files (x86)\\SoftHSM2\\lib\\softhsm2.dll",
];

const TEST_SO_PIN: &str = "12345678";
const TEST_USER_PIN: &str = "1234";
const TEST_TOKEN_LABEL: &str = "icp-cli-test";

/// Find the SoftHSM2 library path
fn find_softhsm_library() -> PathBuf {
    if let Ok(path) = env::var("ICP_CLI_TEST_SOFTHSM_PATH") {
        let path = PathBuf::from(path);
        if path.exists() {
            return path;
        }
        panic!(
            "ICP_CLI_TEST_SOFTHSM_PATH is set but path does not exist: {}",
            path
        );
    }

    for path in DEFAULT_SOFTHSM_PATHS {
        let path = PathBuf::from(*path);
        if path.exists() {
            return path;
        }
    }

    panic!(
        "SoftHSM2 not found. Searched paths: {:?}. \
         Set ICP_CLI_TEST_SOFTHSM_PATH to specify a custom location.",
        DEFAULT_SOFTHSM_PATHS
    );
}

/// Global SoftHSM state shared by all tests in the process.
/// This is initialized once via OnceLock before any threads could read SOFTHSM2_CONF.
struct GlobalSoftHsmState {
    library_path: PathBuf,
    config_path: PathBuf,
    // Keep the temp dir alive for the duration of the process
    _token_dir: camino_tempfile::Utf8TempDir,
}

static GLOBAL_SOFTHSM: OnceLock<GlobalSoftHsmState> = OnceLock::new();

/// Initialize the global SoftHSM environment.
///
/// This must be called at the start of any test that uses SoftHSM, before
/// any concurrent operations. It safely sets SOFTHSM2_CONF once for the process.
fn ensure_global_softhsm_initialized() -> &'static GlobalSoftHsmState {
    GLOBAL_SOFTHSM.get_or_init(|| {
        let library_path = find_softhsm_library();

        // Create temp directory for tokens (lives for entire process)
        let token_dir =
            camino_tempfile::tempdir().expect("failed to create temp dir for SoftHSM tokens");
        let tokens_path = token_dir.path().join("tokens");
        fs::create_dir_all(&tokens_path).expect("failed to create tokens directory");

        // Create softhsm2.conf
        let config_path = token_dir.path().join("softhsm2.conf");
        let config_content = format!(
            "directories.tokendir = {}\nobjectstore.backend = file\nlog.level = ERROR\n",
            tokens_path
        );
        fs::write(&config_path, config_content).expect("failed to write softhsm2.conf");

        // SAFETY: This runs exactly once via OnceLock, before any other code in this
        // process could be reading SOFTHSM2_CONF. The OnceLock guarantees this
        // initialization completes before any caller proceeds.
        unsafe { env::set_var("SOFTHSM2_CONF", &config_path) };

        // Initialize PKCS#11 and create the token
        let pkcs11 = Pkcs11::new(&library_path).expect("failed to load SoftHSM2 PKCS#11 library");
        pkcs11
            .initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))
            .expect("failed to initialize PKCS#11");

        let all_slots = pkcs11.get_all_slots().expect("failed to get slots");
        let slot = all_slots.first().copied().expect("no slots available");

        // Initialize the token
        pkcs11
            .init_token(slot, &AuthPin::new(TEST_SO_PIN.into()), TEST_TOKEN_LABEL)
            .expect("failed to initialize token");

        // Set user PIN
        let session = pkcs11
            .open_rw_session(slot)
            .expect("failed to open session");
        session
            .login(UserType::So, Some(&AuthPin::new(TEST_SO_PIN.into())))
            .expect("failed to login as SO");
        session
            .init_pin(&AuthPin::new(TEST_USER_PIN.into()))
            .expect("failed to set user PIN");
        session.logout().expect("failed to logout");

        GlobalSoftHsmState {
            library_path,
            config_path,
            _token_dir: token_dir,
        }
    })
}

/// A SoftHSM context for testing HSM identity functionality.
///
/// Each instance generates a unique key on the shared token.
pub struct SoftHsmContext {
    /// Path to the SoftHSM2 library
    pub library_path: PathBuf,
    /// Path to the softhsm2.conf file
    pub config_path: PathBuf,
    /// The slot index where the test token was initialized
    pub slot_index: usize,
    /// The key ID of the generated test key (unique per context)
    pub key_id: String,
    /// The user PIN for accessing the token
    pub user_pin: String,
}

static KEY_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl SoftHsmContext {
    /// Create a new SoftHSM test context with a unique key.
    ///
    /// This initializes the global SoftHSM environment if needed, then
    /// generates a unique ECDSA P-256 key pair for this test.
    pub fn new() -> Self {
        let global = ensure_global_softhsm_initialized();

        // Generate a unique key ID for this test instance
        let key_num = KEY_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let key_id_bytes = key_num.to_be_bytes();
        let key_id_hex = hex::encode(key_id_bytes);

        // Connect to the token and generate a key
        let pkcs11 =
            Pkcs11::new(&global.library_path).expect("failed to load SoftHSM2 PKCS#11 library");
        pkcs11
            .initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))
            .expect("failed to initialize PKCS#11");

        let all_slots = pkcs11.get_all_slots().expect("failed to get slots");
        let slot = all_slots.first().copied().expect("no slots available");

        let session = pkcs11
            .open_rw_session(slot)
            .expect("failed to open session");
        session
            .login(UserType::User, Some(&AuthPin::new(TEST_USER_PIN.into())))
            .expect("failed to login as user");

        let pub_template = vec![
            Attribute::Token(true),
            Attribute::Private(false),
            Attribute::Id(key_id_bytes.to_vec()),
            Attribute::Label(format!("test-key-{}", key_num).into_bytes()),
            Attribute::EcParams(
                // OID for P-256 (prime256v1/secp256r1): 1.2.840.10045.3.1.7
                vec![0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07],
            ),
        ];

        let priv_template = vec![
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::Sensitive(true),
            Attribute::Extractable(false),
            Attribute::Id(key_id_bytes.to_vec()),
            Attribute::Label(format!("test-key-{}", key_num).into_bytes()),
            Attribute::Sign(true),
        ];

        session
            .generate_key_pair(&Mechanism::EccKeyPairGen, &pub_template, &priv_template)
            .expect("failed to generate key pair");

        session.logout().expect("failed to logout");

        let slot_index = all_slots
            .iter()
            .position(|s| *s == slot)
            .expect("slot not found");

        Self {
            library_path: global.library_path.clone(),
            config_path: global.config_path.clone(),
            slot_index,
            key_id: key_id_hex,
            user_pin: TEST_USER_PIN.to_string(),
        }
    }

    /// Get the library path as a string for CLI arguments
    pub fn library_path_str(&self) -> &str {
        self.library_path.as_str()
    }
}
