use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde::Serialize;
use snafu::prelude::*;
use tokio::sync::Mutex;
use tracing::debug;

use crate::{
    canister::{Settings, recipe::Resolve},
    manifest::{
        LoadManifestFromPathError, PROJECT_MANIFEST, ProjectRootLocate, ProjectRootLocateError,
        canister::{BuildSteps, SyncSteps},
        load_manifest_from_path,
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
pub mod package;
pub mod prelude;
pub mod project;
pub mod settings;
pub mod store_artifact;
pub mod store_id;

const ICP_BASE: &str = ".icp";
const CACHE_DIR: &str = "cache";
const DATA_DIR: &str = "data";

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
    /// Can be hex-encoded bytes or Candid text format.
    pub init_args: Option<String>,
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

/// Consolidated project definition
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

#[derive(Debug, Snafu)]
pub enum ProjectLoadError {
    #[snafu(display("failed to locate project directory"))]
    Locate { source: ProjectRootLocateError },

    #[snafu(display("failed to load project manifest"))]
    ProjectManifest { source: LoadManifestFromPathError },

    #[snafu(display("failed to load project"))]
    Project {
        source: project::ConsolidateManifestError,
    },
}

#[async_trait]
pub trait ProjectLoad: Sync + Send {
    async fn load(&self) -> Result<Project, ProjectLoadError>;
    async fn exists(&self) -> Result<bool, ProjectLoadError>;
}

pub struct ProjectLoadImpl {
    pub project_root_locate: Arc<dyn ProjectRootLocate>,
    pub recipe: Arc<dyn Resolve>,
}

#[async_trait]
impl ProjectLoad for ProjectLoadImpl {
    async fn load(&self) -> Result<Project, ProjectLoadError> {
        debug!("Loading project");
        // Locate project root
        let pdir = self.project_root_locate.locate().context(LocateSnafu)?;

        debug!("Located icp project in {pdir}");

        // Load project manifest
        let m = load_manifest_from_path(&pdir.join(PROJECT_MANIFEST))
            .await
            .context(ProjectManifestSnafu)?;

        debug!("Loaded project manifest: {m:#?}");

        // Consolidate manifest into project
        let p = project::consolidate_manifest(&pdir, self.recipe.as_ref(), &m)
            .await
            .context(ProjectSnafu)?;

        debug!("Rendered project definition: {p:#?}");

        Ok(p)
    }

    async fn exists(&self) -> Result<bool, ProjectLoadError> {
        match self.project_root_locate.locate() {
            Ok(_) => Ok(true),
            Err(ProjectRootLocateError::NotFound { .. }) => Ok(false),
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
impl<T: ProjectLoad> ProjectLoad for Lazy<T, Project> {
    async fn load(&self) -> Result<Project, ProjectLoadError> {
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

    async fn exists(&self) -> Result<bool, ProjectLoadError> {
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
    ///   - "ic" (connected to the IC mainnet)
    /// - Environments:
    ///   - "dev" (local network, all three canisters)
    ///   - "test" (staging network, backend and frontend only)
    ///   - "prod" (ic network, backend and frontend only)
    pub fn complex() -> Self {
        use crate::{
            context::IC_ROOT_KEY,
            manifest::{
                adapter::prebuilt::{Adapter as PrebuiltAdapter, LocalSource, SourceField},
                canister::{BuildStep, BuildSteps, SyncSteps},
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
        };

        // Create networks
        let local_network = Network {
            name: "local".to_string(),
            configuration: Configuration::Managed {
                managed: Managed {
                    mode: ManagedMode::Launcher(Box::new(ManagedLauncherConfig {
                        gateway: Gateway {
                            host: "localhost".to_string(),
                            port: Port::Fixed(8000),
                        },
                        artificial_delay_ms: None,
                        ii: false,
                        nns: false,
                        subnets: None,
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
                            host: "localhost".to_string(),
                            port: Port::Fixed(8001),
                        },
                        artificial_delay_ms: None,
                        ii: false,
                        nns: false,
                        subnets: None,
                        version: None,
                    })),
                },
            },
        };

        let ic_network = Network {
            name: "ic".to_string(),
            configuration: Configuration::Connected {
                connected: Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: IC_ROOT_KEY.to_vec(),
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
impl ProjectLoad for MockProjectLoader {
    async fn load(&self) -> Result<Project, ProjectLoadError> {
        Ok(self.project.clone())
    }

    async fn exists(&self) -> Result<bool, ProjectLoadError> {
        Ok(true)
    }
}

#[cfg(test)]
/// Mock project loader that always fails with a Locate error.
/// Useful for testing scenarios where no project exists.
pub struct NoProjectLoader;

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canister::recipe::{Resolve, ResolveError};
    use crate::manifest::{
        ProjectRootLocate, ProjectRootLocateError,
        canister::{BuildSteps, SyncSteps},
        recipe::Recipe,
    };
    use camino_tempfile::Utf8TempDir;
    use indoc::indoc;

    struct MockProjectRootLocate {
        path: PathBuf,
    }

    impl MockProjectRootLocate {
        fn new(path: PathBuf) -> Self {
            Self { path }
        }
    }

    impl ProjectRootLocate for MockProjectRootLocate {
        fn locate(&self) -> Result<PathBuf, ProjectRootLocateError> {
            Ok(self.path.clone())
        }
    }

    struct MockRecipeResolver;

    #[async_trait]
    impl Resolve for MockRecipeResolver {
        async fn resolve(&self, _recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
            use crate::manifest::adapter::prebuilt::{
                Adapter as PrebuiltAdapter, LocalSource, SourceField,
            };
            use crate::manifest::canister::BuildStep;

            // Create a minimal BuildSteps with a dummy prebuilt step
            let build_steps = BuildSteps {
                steps: vec![BuildStep::Prebuilt(PrebuiltAdapter {
                    source: SourceField::Local(LocalSource {
                        path: "dummy.wasm".into(),
                    }),
                    sha256: None,
                })],
            };

            Ok((build_steps, SyncSteps::default()))
        }
    }

    #[tokio::test]
    async fn test_load_minimal_project() {
        // Create temp directory with icp.yaml
        let temp_dir = Utf8TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Write a minimal icp.yaml
        let manifest_content = indoc! {r#"
            canisters:
              - name: backend
                build:
                  steps:
                    - type: pre-built
                      path: backend.wasm
        "#};
        std::fs::write(project_dir.join("icp.yaml"), manifest_content).unwrap();

        // Create ProjectLoadImpl with mocks
        let loader = ProjectLoadImpl {
            project_root_locate: Arc::new(MockProjectRootLocate::new(project_dir.to_path_buf())),
            recipe: Arc::new(MockRecipeResolver),
        };

        // Call load
        let result = loader.load().await;

        // Assert success and check project contents
        assert!(result.is_ok());
        let project = result.unwrap();
        assert_eq!(project.dir, project_dir);
        assert!(
            project.canisters.contains_key("backend"),
            "The backend canister was not found"
        );
        assert!(
            project.environments.contains_key("local"),
            "The default `local` environment was not injected"
        );
        assert!(
            project.environments.contains_key("ic"),
            "The default `ic` environment was not injected"
        );
        assert!(
            project.networks.contains_key("local"),
            "The default `local` network was not injected"
        );
        assert!(
            project.networks.contains_key("ic"),
            "The default `ic` network was not injected"
        );
    }

    #[tokio::test]
    async fn test_load_project_local_override() {
        // Create temp directory with icp.yaml
        let temp_dir = Utf8TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Write a minimal icp.yaml
        let manifest_content = indoc! {r#"
            networks:
              - name: test-network
                mode: connected
                url: https://somenetwork.icp
            environments:
              - name: local
                network: test-network
            canisters:
              - name: backend
                build:
                  steps:
                    - type: pre-built
                      path: backend.wasm
        "#};
        std::fs::write(project_dir.join("icp.yaml"), manifest_content).unwrap();

        // Create ProjectLoadImpl with mocks
        let loader = ProjectLoadImpl {
            project_root_locate: Arc::new(MockProjectRootLocate::new(project_dir.to_path_buf())),
            recipe: Arc::new(MockRecipeResolver),
        };

        // Call load
        let result = loader.load().await;

        // Assert success and check project contents
        assert!(result.is_ok(), "The project did not load: {:?}", result);
        let project = result.unwrap();
        assert_eq!(project.dir, project_dir);
        assert!(
            project.canisters.contains_key("backend"),
            "The backend canister was not found"
        );
        assert!(
            project.environments.contains_key("local"),
            "The default `local` environment was not injected"
        );
        let e = project.environments.get("local").unwrap();
        assert_eq!(e.network.name, "test-network");
        assert!(
            project.environments.contains_key("ic"),
            "The default `ic` environment was not injected"
        );
        assert!(
            project.networks.contains_key("local"),
            "The default `local` network was not injected"
        );
        assert!(
            project.networks.contains_key("ic"),
            "The default `ic` network was not injected"
        );
    }
}
