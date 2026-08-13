use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use indexmap::IndexMap;
use serde::Serialize;
use snafu::prelude::*;

use candid_parser::parse_idl_args;

use crate::{
    canister::Settings,
    manifest::{
        ArgsFormat, ProjectRootLocateError,
        canister::{BuildSteps, SyncSteps},
    },
    network::Configuration,
    prelude::*,
};

pub mod agent;
pub mod canister;
pub mod context;
pub mod directories;
pub mod fs;
pub mod manifest;
pub mod network;
pub mod package;
pub mod parsers;
pub mod prelude;
pub mod settings;
pub mod signal;
pub mod store_artifact;
pub mod store_id;
pub mod telemetry_data;

const ICP_BASE: &str = ".icp";
const CACHE_DIR: &str = "cache";
const DATA_DIR: &str = "data";

/// Resolved initialization arguments, with any file references already loaded.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum InitArgs {
    /// Text content (inline or loaded from file). Format is always known.
    Text { content: String, format: ArgsFormat },
    /// Raw binary bytes (from a file with `format: bin`). Used directly.
    Binary(Vec<u8>),
}

#[derive(Debug, Snafu)]
pub enum InitArgsToBytesError {
    #[snafu(display("failed to decode hex init args"))]
    HexDecode { source: hex::FromHexError },

    #[snafu(display("failed to parse Candid init args"))]
    CandidParse { source: candid_parser::Error },

    #[snafu(display("failed to encode Candid init args to bytes"))]
    CandidEncode { source: candid::Error },
}

impl InitArgs {
    /// Resolve to raw bytes according to the format.
    pub fn to_bytes(&self) -> Result<Vec<u8>, InitArgsToBytesError> {
        match self {
            InitArgs::Binary(bytes) => Ok(bytes.clone()),
            InitArgs::Text { content, format } => match format {
                ArgsFormat::Hex => hex::decode(content.trim()).context(HexDecodeSnafu),
                ArgsFormat::Candid => {
                    let args = parse_idl_args(content.trim()).context(CandidParseSnafu)?;
                    args.to_bytes().context(CandidEncodeSnafu)
                }
                ArgsFormat::Bin => {
                    unreachable!("binary format cannot appear in InitArgs::Text")
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Canister {
    pub name: String,

    /// Canister settings, such as memory constaints, etc.
    pub settings: Settings,

    /// The build configuration specifying how to compile the canister's source
    /// code into a WebAssembly module, including the adapter to use.
    pub build: BuildSteps,

    /// The configuration specifying how to sync the canister
    pub sync: SyncSteps,

    /// Initialization arguments passed to the canister during installation.
    /// Resolved from the manifest — file contents are already loaded.
    pub init_args: Option<InitArgs>,

    /// If the canister was defined via a recipe reference, this holds the
    /// original recipe specifier string (e.g. `@dfinity/motoko@v4.0.0`).
    /// `None` when the canister uses explicit build/sync instructions.
    pub registry_recipe: Option<String>,

    /// Canister-discovery wiring. Maps the name this canister reads in a
    /// `PUBLIC_CANISTER_ID:<name>` environment variable to the store key of the
    /// referenced canister. Computed during consolidation so each canister sees
    /// the view its owning project expects: its own project's canisters under
    /// their local names, plus any declared dependencies under their aliases
    /// (`<alias>:<canister>`). For a project with no dependencies this maps every
    /// canister's local name to itself, reproducing the flat "every canister sees
    /// every sibling" behavior.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,

    /// Subdomain prefixes for the canister's friendly URLs, most-specific label
    /// first, e.g. `["backend"]` for an own canister or `["backend.openemail"]`
    /// for a dependency canister (dot-nested by alias chain). A de-duplicated
    /// shared dependency canister carries one entry per alias chain that reaches
    /// it. Consumed only at deploy time to build `custom-domains.txt` entries and
    /// the printed URLs; a runtime display aid that is always recomputed during
    /// consolidation, so it is never serialized.
    #[serde(skip)]
    pub friendly_names: Vec<String>,

    /// For each environment variable whose value came from a file, the file it
    /// was read from. `settings.environment_variables` already holds the
    /// contents; the paths are kept so `icp project bundle` can hold a file
    /// backing a variable to the same containment rule it applies to every other
    /// file a manifest points at. Bookkeeping for that check rather than part of
    /// the resolved configuration, so it is never serialized.
    #[serde(skip)]
    pub environment_variable_files: BTreeMap<String, PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Network {
    pub name: String,
    pub configuration: Configuration,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Environment {
    pub name: String,
    pub network: Network,
    pub canisters: IndexMap<String, (PathBuf, Canister)>,
}

impl Environment {
    pub fn get_canister_names(&self) -> Vec<String> {
        self.canisters.keys().cloned().collect()
    }

    pub fn contains_canister(&self, canister_name: &str) -> bool {
        self.canisters.contains_key(canister_name)
    }

    pub fn get_canister_info(&self, canister: &str) -> Result<(PathBuf, Canister), String> {
        self.canisters
            .get(canister)
            .ok_or_else(|| {
                format!(
                    "canister '{}' not declared in environment '{}'",
                    canister, self.name
                )
            })
            .cloned()
    }
}

/// Consolidated project definition
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Project {
    pub dir: PathBuf,
    pub canisters: IndexMap<String, (PathBuf, Canister)>,
    pub networks: HashMap<String, Network>,
    pub environments: HashMap<String, Environment>,

    /// Environments the workspace defines that some vendored member does *not*
    /// declare, keyed by environment name → the missing members' store-key
    /// prefixes. Enforced when the environment is selected (strict rule).
    /// Empty for standalone projects and workspaces whose members are complete.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub member_missing_envs: HashMap<String, Vec<String>>,
}

impl Project {
    pub fn get_canister(&self, canister_name: &str) -> Option<&(PathBuf, Canister)> {
        self.canisters.get(canister_name)
    }
}

#[derive(Debug, Snafu)]
pub enum ProjectLoadError {
    #[snafu(display("failed to locate project directory"))]
    Locate { source: ProjectRootLocateError },

    /// Anything that went wrong inside a [`ProjectLoad`] implementation, e.g.
    /// reading and consolidating manifests in the CLI's native loader. Boxed so
    /// that the library does not depend on any one loader's error type; the
    /// implementation's own error is the reported cause.
    #[snafu(transparent)]
    Load {
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

#[async_trait]
pub trait ProjectLoad: Sync + Send {
    async fn load(&self) -> Result<Project, ProjectLoadError>;
    async fn exists(&self) -> Result<bool, ProjectLoadError>;

    /// The directory of the member (sub-project) the command is standing in,
    /// i.e. the nearest `icp.yaml` at or above cwd. Equals the workspace root
    /// (`Project::dir`) at the root or in a standalone project. `None` when the
    /// member directory cannot be determined; callers then skip member-scoping.
    fn member_dir(&self) -> Option<PathBuf> {
        None
    }
}

#[cfg(any(test, feature = "mocks"))]
/// Mock project loader for testing.
/// Returns a pre-configured `Project` when `load()` is called.
pub struct MockProjectLoader {
    project: Project,
}

#[cfg(any(test, feature = "mocks"))]
impl MockProjectLoader {
    /// Creates a new mock project loader with the given project.
    pub fn new(project: Project) -> Self {
        Self { project }
    }

    /// Creates a minimal project with one canister, one network, and one environment.
    ///
    /// Structure:
    /// - Canister: "backend" (pre-built from local file "backend.wasm")
    /// - Network: "local" (managed, localhost:8000)
    /// - Environment: "default" (uses local network, includes backend canister)
    pub fn minimal() -> Self {
        use crate::{
            manifest::adapter::prebuilt::{Adapter as PrebuiltAdapter, LocalSource, SourceField},
            manifest::canister::{BuildStep, BuildSteps, SyncSteps},
            network::{Configuration, Managed},
        };

        let backend_canister = Canister {
            name: "backend".to_string(),
            settings: Settings::default(),
            build: BuildSteps {
                steps: vec![BuildStep::Prebuilt(PrebuiltAdapter {
                    source: SourceField::Local(LocalSource {
                        path: "backend.wasm".into(),
                    }),
                    sha256: None,
                })],
            },
            sync: SyncSteps::default(),
            init_args: None,
            registry_recipe: None,
            bindings: BTreeMap::new(),
            friendly_names: vec!["backend".to_string()],
            environment_variable_files: BTreeMap::new(),
        };

        let local_network = Network {
            name: "local".to_string(),
            configuration: Configuration::Managed {
                managed: Managed::default(),
            },
        };

        let mut canisters = IndexMap::new();
        canisters.insert(
            "backend".to_string(),
            ("/project".into(), backend_canister.clone()),
        );

        let mut networks = HashMap::new();
        networks.insert("local".to_string(), local_network.clone());

        let mut env_canisters = IndexMap::new();
        env_canisters.insert("backend".to_string(), ("/project".into(), backend_canister));

        let default_env = Environment {
            name: "default".to_string(),
            network: local_network,
            canisters: env_canisters,
        };

        let mut environments = HashMap::new();
        environments.insert("default".to_string(), default_env);

        let project = Project {
            dir: "/project".into(),
            canisters,
            networks,
            environments,
            member_missing_envs: HashMap::new(),
        };

        Self::new(project)
    }

    /// Creates a complex project with multiple canisters, networks, and environments.
    ///
    /// Structure:
    /// - Canisters:
    ///   - "backend" (pre-built from local "backend.wasm")
    ///   - "frontend" (pre-built from local "frontend.wasm")
    ///   - "database" (pre-built from local "database.wasm")
    /// - Networks:
    ///   - "local" (managed, localhost:8000)
    ///   - "staging" (managed, localhost:8001)
    ///   - "ic" (connected to the IC mainnet)
    /// - Environments:
    ///   - "dev" (local network, all three canisters)
    ///   - "test" (staging network, backend and frontend only)
    ///   - "prod" (ic network, backend and frontend only)
    pub fn complex() -> Self {
        use crate::{
            manifest::{
                adapter::prebuilt::{Adapter as PrebuiltAdapter, LocalSource, SourceField},
                canister::{BuildStep, BuildSteps, SyncSteps},
                network::RootKeySpec,
            },
            network::{
                Configuration, Connected, Gateway, Managed, ManagedLauncherConfig, ManagedMode,
                Port,
            },
        };

        // Create canisters
        let backend_canister = Canister {
            name: "backend".to_string(),
            settings: Settings::default(),
            build: BuildSteps {
                steps: vec![BuildStep::Prebuilt(PrebuiltAdapter {
                    source: SourceField::Local(LocalSource {
                        path: "backend.wasm".into(),
                    }),
                    sha256: None,
                })],
            },
            sync: SyncSteps::default(),
            init_args: None,
            registry_recipe: None,
            bindings: BTreeMap::new(),
            friendly_names: vec!["backend".to_string()],
            environment_variable_files: BTreeMap::new(),
        };

        let frontend_canister = Canister {
            name: "frontend".to_string(),
            settings: Settings::default(),
            build: BuildSteps {
                steps: vec![BuildStep::Prebuilt(PrebuiltAdapter {
                    source: SourceField::Local(LocalSource {
                        path: "frontend.wasm".into(),
                    }),
                    sha256: None,
                })],
            },
            sync: SyncSteps::default(),
            init_args: None,
            registry_recipe: None,
            bindings: BTreeMap::new(),
            friendly_names: vec!["frontend".to_string()],
            environment_variable_files: BTreeMap::new(),
        };

        let database_canister = Canister {
            name: "database".to_string(),
            settings: Settings::default(),
            build: BuildSteps {
                steps: vec![BuildStep::Prebuilt(PrebuiltAdapter {
                    source: SourceField::Local(LocalSource {
                        path: "database.wasm".into(),
                    }),
                    sha256: None,
                })],
            },
            sync: SyncSteps::default(),
            init_args: None,
            registry_recipe: None,
            bindings: BTreeMap::new(),
            friendly_names: vec!["database".to_string()],
            environment_variable_files: BTreeMap::new(),
        };

        // Create networks
        let local_network = Network {
            name: "local".to_string(),
            configuration: Configuration::Managed {
                managed: Managed {
                    mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                        gateway: Gateway {
                            bind: "127.0.0.1".to_string(),
                            port: Port::Fixed(8000),
                            domains: vec![],
                        },
                        artificial_delay_ms: None,
                        ii: false,
                        nns: false,
                        subnets: None,
                        bitcoind_addr: None,
                        dogecoind_addr: None,
                        version: None,
                    })),
                },
            },
        };

        let staging_network = Network {
            name: "staging".to_string(),
            configuration: Configuration::Managed {
                managed: Managed {
                    mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                        gateway: Gateway {
                            bind: "127.0.0.1".to_string(),
                            port: Port::Fixed(8001),
                            domains: vec![],
                        },
                        artificial_delay_ms: None,
                        ii: false,
                        nns: false,
                        subnets: None,
                        bitcoind_addr: None,
                        dogecoind_addr: None,
                        version: None,
                    })),
                },
            },
        };

        let ic_network = Network {
            name: "ic".to_string(),
            configuration: Configuration::Connected {
                connected: Connected {
                    api_url: IC_MAINNET_NETWORK_API_URL.parse().unwrap(),
                    http_gateway_url: Some(IC_MAINNET_NETWORK_GATEWAY_URL.parse().unwrap()),
                    root_key: RootKeySpec::Mainnet,
                },
            },
        };

        // Setup canisters map
        let mut canisters = IndexMap::new();
        canisters.insert(
            "backend".to_string(),
            ("/project/backend".into(), backend_canister.clone()),
        );
        canisters.insert(
            "frontend".to_string(),
            ("/project/frontend".into(), frontend_canister.clone()),
        );
        canisters.insert(
            "database".to_string(),
            ("/project/database".into(), database_canister.clone()),
        );

        // Setup networks map
        let mut networks = HashMap::new();
        networks.insert("local".to_string(), local_network.clone());
        networks.insert("staging".to_string(), staging_network.clone());
        networks.insert("ic".to_string(), ic_network.clone());

        // Create dev environment (all canisters on local)
        let mut dev_canisters = IndexMap::new();
        dev_canisters.insert(
            "backend".to_string(),
            ("/project/backend".into(), backend_canister.clone()),
        );
        dev_canisters.insert(
            "frontend".to_string(),
            ("/project/frontend".into(), frontend_canister.clone()),
        );
        dev_canisters.insert(
            "database".to_string(),
            ("/project/database".into(), database_canister.clone()),
        );

        let dev_env = Environment {
            name: "dev".to_string(),
            network: local_network,
            canisters: dev_canisters,
        };

        // Create test environment (backend and frontend on staging)
        let mut test_canisters = IndexMap::new();
        test_canisters.insert(
            "backend".to_string(),
            ("/project/backend".into(), backend_canister.clone()),
        );
        test_canisters.insert(
            "frontend".to_string(),
            ("/project/frontend".into(), frontend_canister.clone()),
        );

        let test_env = Environment {
            name: "test".to_string(),
            network: staging_network,
            canisters: test_canisters,
        };

        // Create prod environment (backend and frontend on ic)
        let mut prod_canisters = IndexMap::new();
        prod_canisters.insert(
            "backend".to_string(),
            ("/project/backend".into(), backend_canister),
        );
        prod_canisters.insert(
            "frontend".to_string(),
            ("/project/frontend".into(), frontend_canister),
        );

        let prod_env = Environment {
            name: "prod".to_string(),
            network: ic_network,
            canisters: prod_canisters,
        };

        // Setup environments map
        let mut environments = HashMap::new();
        environments.insert("dev".to_string(), dev_env);
        environments.insert("test".to_string(), test_env);
        environments.insert("prod".to_string(), prod_env);

        let project = Project {
            dir: "/project".into(),
            canisters,
            networks,
            environments,
            member_missing_envs: HashMap::new(),
        };

        Self::new(project)
    }
}

#[cfg(any(test, feature = "mocks"))]
#[async_trait]
impl ProjectLoad for MockProjectLoader {
    async fn load(&self) -> Result<Project, ProjectLoadError> {
        Ok(self.project.clone())
    }

    async fn exists(&self) -> Result<bool, ProjectLoadError> {
        Ok(true)
    }
}

#[cfg(any(test, feature = "mocks"))]
/// Mock project loader that always fails with a Locate error.
/// Useful for testing scenarios where no project exists.
pub struct NoProjectLoader;

#[cfg(any(test, feature = "mocks"))]
#[async_trait]
impl ProjectLoad for NoProjectLoader {
    async fn load(&self) -> Result<Project, ProjectLoadError> {
        Err(ProjectLoadError::Locate {
            source: ProjectRootLocateError::NotFound {
                path: "/some/path".into(),
            },
        })
    }

    async fn exists(&self) -> Result<bool, ProjectLoadError> {
        Ok(false)
    }
}
