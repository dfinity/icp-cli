use std::{collections::HashMap, sync::Arc};

use anyhow::Context;
use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::Mutex;
use tracing::debug;

use crate::{
    canister::{Settings, build, sync},
    manifest::{
        PROJECT_MANIFEST, ProjectRootLocate, ProjectRootLocateError, project::ProjectManifest,
    },
    network::Configuration,
    prelude::*,
};

pub mod agent;
pub mod canister;
pub mod context;
pub mod directories;
pub mod fs;
pub mod identity;
pub mod manifest;
pub mod network;
pub mod prelude;
pub mod project;
pub mod store_artifact;
pub mod store_id;

const ICP_BASE: &str = ".icp";
const CACHE_DIR: &str = "cache";
const DATA_DIR: &str = "data";

fn is_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{')
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Canister {
    pub name: String,

    /// Canister settings, such as memory constaints, etc.
    pub settings: Settings,

    /// The build configuration specifying how to compile the canister's source
    /// code into a WebAssembly module, including the adapter to use.
    pub build: build::Steps,

    /// The configuration specifying how to sync the canister
    pub sync: sync::Steps,
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
    pub canisters: HashMap<String, (PathBuf, Canister)>,
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Project {
    pub dir: PathBuf,
    pub canisters: HashMap<String, (PathBuf, Canister)>,
    pub networks: HashMap<String, Network>,
    pub environments: HashMap<String, Environment>,
}

impl Project {
    pub fn get_canister(&self, canister_name: &str) -> Option<&(PathBuf, Canister)> {
        self.canisters.get(canister_name)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("failed to locate project directory")]
    Locate,

    #[error("failed to load path")]
    Path,

    #[error("failed to load the project manifest")]
    Manifest,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Load: Sync + Send {
    async fn load(&self) -> Result<Project, LoadError>;
    async fn exists(&self) -> Result<bool, LoadError>;
}

#[async_trait]
pub trait LoadPath<M, E>: Sync + Send {
    async fn load(&self, path: &Path) -> Result<M, E>;
}

#[async_trait]
pub trait LoadManifest<M, T, E>: Sync + Send {
    async fn load(&self, m: &M) -> Result<T, E>;
}

pub struct ProjectLoaders {
    pub path: Arc<dyn LoadPath<ProjectManifest, project::LoadPathError>>,
    pub manifest: Arc<dyn LoadManifest<ProjectManifest, Project, project::LoadManifestError>>,
}

pub struct Loader {
    pub project_root_locate: Arc<dyn ProjectRootLocate>,
    pub project: ProjectLoaders,
}

#[async_trait]
impl Load for Loader {
    async fn load(&self) -> Result<Project, LoadError> {
        debug!("Loading project");
        // Locate project root
        let pdir = self
            .project_root_locate
            .locate()
            .context(LoadError::Locate)?;

        debug!("Located icp project in {pdir}");

        // Load project manifest
        let m = self
            .project
            .path
            .load(&pdir.join(PROJECT_MANIFEST))
            .await
            .context(LoadError::Path)?;

        debug!("Loaded project manifest: {m:#?}");

        // Load project
        let p = self
            .project
            .manifest
            .load(&m)
            .await
            .context(LoadError::Manifest)?;

        debug!("Rendered project definition: {p:#?}");

        Ok(p)
    }

    async fn exists(&self) -> Result<bool, LoadError> {
        match self.project_root_locate.locate() {
            Ok(_) => Ok(true),
            Err(ProjectRootLocateError::NotFound(_)) => Ok(false),
            Err(ProjectRootLocateError::Unexpected(e)) => Err(LoadError::Unexpected(e)),
        }
    }
}

pub struct Lazy<T, V>(T, Arc<Mutex<Option<V>>>);

impl<T, V> Lazy<T, V> {
    pub fn new(v: T) -> Self {
        Self(v, Arc::new(Mutex::new(None)))
    }
}

#[async_trait]
impl<T: Load> Load for Lazy<T, Project> {
    async fn load(&self) -> Result<Project, LoadError> {
        if let Some(v) = self.1.lock().await.as_ref() {
            return Ok(v.to_owned());
        }

        let v = self.0.load().await?;

        let mut g = self.1.lock().await;
        if g.is_none() {
            *g = Some(v.to_owned());
        }

        Ok(v)
    }

    async fn exists(&self) -> Result<bool, LoadError> {
        if self.1.lock().await.as_ref().is_some() {
            return Ok(true);
        }

        let v = self.0.exists().await?;
        Ok(v)
    }
}

#[cfg(test)]
/// Mock project loader for testing.
/// Returns a pre-configured `Project` when `load()` is called.
pub struct MockProjectLoader {
    project: Project,
}

#[cfg(test)]
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
            canister::build::{Step as BuildStep, Steps as BuildSteps},
            canister::sync::Steps as SyncSteps,
            manifest::adapter::prebuilt::{Adapter as PrebuiltAdapter, LocalSource, SourceField},
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
        };

        let local_network = Network {
            name: "local".to_string(),
            configuration: Configuration::Managed {
                managed: Managed::default(),
            },
        };

        let mut canisters = HashMap::new();
        canisters.insert(
            "backend".to_string(),
            ("/project".into(), backend_canister.clone()),
        );

        let mut networks = HashMap::new();
        networks.insert("local".to_string(), local_network.clone());

        let mut env_canisters = HashMap::new();
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
    ///   - "ic" (connected to mainnet)
    /// - Environments:
    ///   - "dev" (local network, all three canisters)
    ///   - "test" (staging network, backend and frontend only)
    ///   - "prod" (ic network, backend and frontend only)
    pub fn complex() -> Self {
        use crate::{
            canister::build::{Step as BuildStep, Steps as BuildSteps},
            canister::sync::Steps as SyncSteps,
            manifest::adapter::prebuilt::{Adapter as PrebuiltAdapter, LocalSource, SourceField},
            network::{Configuration, Connected, Gateway, Managed, Port},
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
        };

        // Create networks
        let local_network = Network {
            name: "local".to_string(),
            configuration: Configuration::Managed {
                managed: Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8000),
                    },
                },
            },
        };

        let staging_network = Network {
            name: "staging".to_string(),
            configuration: Configuration::Managed {
                managed: Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: Port::Fixed(8001),
                    },
                },
            },
        };

        let ic_network = Network {
            name: "ic".to_string(),
            configuration: Configuration::Connected {
                connected: Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: None,
                },
            },
        };

        // Setup canisters map
        let mut canisters = HashMap::new();
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
        let mut dev_canisters = HashMap::new();
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
        let mut test_canisters = HashMap::new();
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
        let mut prod_canisters = HashMap::new();
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
        };

        Self::new(project)
    }
}

#[cfg(test)]
#[async_trait]
impl Load for MockProjectLoader {
    async fn load(&self) -> Result<Project, LoadError> {
        Ok(self.project.clone())
    }

    async fn exists(&self) -> Result<bool, LoadError> {
        Ok(true)
    }
}

#[cfg(test)]
/// Mock project loader that always fails with a Locate error.
/// Useful for testing scenarios where no project exists.
pub struct NoProjectLoader;

#[cfg(test)]
#[async_trait]
impl Load for NoProjectLoader {
    async fn load(&self) -> Result<Project, LoadError> {
        Err(LoadError::Locate)
    }

    async fn exists(&self) -> Result<bool, LoadError> {
        Ok(false)
    }
}
