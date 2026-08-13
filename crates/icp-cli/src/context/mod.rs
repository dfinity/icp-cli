//! The CLI's execution context.
//!
//! Wraps the library [`icp::context::Context`] — which is a bag of ports for
//! building and deploying — with the frontend-only state the library has no
//! business knowing about: the presentation flags, the password prompt, and the
//! identity loader. Derefs to the library context, so every library port
//! (`dirs`, `ids`, `project`, `network`, …) is reached straight through it.

use std::{env::current_dir, ops::Deref, sync::Arc, time::Duration};

use ic_agent::{Agent, Identity};
use icp::{
    ProjectLoadError,
    canister::recipe::handlebars::Handlebars,
    context::{EnvironmentSelection, NetworkSelection},
    directories::{Access as _, Directories},
    prelude::*,
};
use snafu::prelude::*;
use url::Url;

use crate::{
    identity::{IdentityDirectories, IdentityPaths, IdentitySelection, PasswordFunc},
    manifest::ProjectRootLocateImpl,
    project::{Lazy, ProjectLoadImpl},
};

/// Execution context for a single CLI invocation.
#[derive(Clone)]
pub struct Context {
    /// The library context.
    inner: icp::context::Context,

    /// Identity loader. Caches per selection, so an encrypted identity is
    /// unlocked (and its password asked for) at most once per invocation.
    identity: Arc<dyn crate::identity::Load>,

    /// Whether debug output is enabled (`--debug`). Presentation only: it
    /// selects the tracing layer and hides progress bars.
    pub debug: bool,

    /// Password reader for identity decryption; shared with the identity loader.
    pub password_func: PasswordFunc,
}

impl Deref for Context {
    type Target = icp::context::Context;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Context {
    /// The identity directory, under its lock.
    pub fn identity_dirs(&self) -> Result<IdentityDirectories, icp::fs::lock::LockError> {
        IdentityPaths::new(self.dirs.identity_dir())
    }

    /// Gets an identity based on the provided identity selection.
    pub async fn get_identity(
        &self,
        identity: &IdentitySelection,
        network_root_key: Option<Vec<u8>>,
    ) -> Result<Arc<dyn Identity>, GetIdentityError> {
        self.identity
            .load(identity.clone(), network_root_key)
            .await
            .context(IdentityLoadSnafu {
                identity: identity.clone(),
            })
    }

    /// Creates an agent for a given identity and environment.
    pub async fn get_agent_for_env(
        &self,
        identity: &IdentitySelection,
        environment: &EnvironmentSelection,
    ) -> Result<Agent, GetAgentForEnvError> {
        let env = self.get_environment(environment).await?;
        // A delegated identity is validated against the network's root key, so
        // the network is resolved before the identity is loaded.
        let access = self.network.access(&env.network).await?;
        let id = self
            .get_identity(identity, Some(access.root_key.clone()))
            .await?;
        Ok(self.create_agent(id, access).await?)
    }

    /// Creates an agent for a given identity and network.
    pub async fn get_agent_for_network(
        &self,
        identity: &IdentitySelection,
        network_selection: &NetworkSelection,
    ) -> Result<Agent, GetAgentForNetworkError> {
        let network = self.get_network(network_selection).await?;
        let access = self.network.access(&network).await?;
        let id = self
            .get_identity(identity, Some(access.root_key.clone()))
            .await?;
        Ok(self.create_agent(id, access).await?)
    }

    /// Creates an agent for a given identity and url.
    pub async fn get_agent_for_url(
        &self,
        identity: &IdentitySelection,
        url: &Url,
    ) -> Result<Agent, GetAgentForUrlError> {
        let id = self.get_identity(identity, None).await?;
        let agent = self.agent.create(id, url.as_str()).await?;
        Ok(agent)
    }

    pub async fn get_agent(
        &self,
        identity: &IdentitySelection,
        network: &NetworkSelection,
        environment: &EnvironmentSelection,
    ) -> Result<Agent, GetAgentError> {
        match (environment, network) {
            // Error: Both environment and network specified
            (EnvironmentSelection::Named(_), NetworkSelection::Named(_))
            | (EnvironmentSelection::Named(_), NetworkSelection::Url(_, _)) => {
                Err(GetAgentError::EnvironmentAndNetworkSpecified)
            }

            // Default environment + default network
            (EnvironmentSelection::Default, NetworkSelection::Default) => {
                // Try to get agent from the default environment if project exists
                match self.get_agent_for_env(identity, environment).await {
                    Ok(agent) => Ok(agent),
                    Err(GetAgentForEnvError::GetEnvironment {
                        source:
                            icp::context::GetEnvironmentError::ProjectLoad {
                                source: ProjectLoadError::Locate { .. },
                            },
                    }) => Err(GetAgentError::NoProjectOrNetwork),
                    Err(e) => Err(e.into()),
                }
            }

            // Environment specified
            (EnvironmentSelection::Named(_), NetworkSelection::Default) => {
                Ok(self.get_agent_for_env(identity, environment).await?)
            }

            // Network specified
            (EnvironmentSelection::Default, NetworkSelection::Named(_))
            | (EnvironmentSelection::Default, NetworkSelection::Url(_, _)) => {
                Ok(self.get_agent_for_network(identity, network).await?)
            }
        }
    }
}

#[derive(Debug, Snafu)]
pub enum GetIdentityError {
    #[snafu(display("failed to load identity"))]
    IdentityLoad {
        source: crate::identity::LoadError,
        identity: IdentitySelection,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForEnvError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    GetEnvironment {
        source: icp::context::GetEnvironmentError,
    },

    #[snafu(transparent)]
    NetworkAccess { source: icp::network::AccessError },

    #[snafu(transparent)]
    AgentCreate {
        source: icp::agent::CreateAgentError,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForNetworkError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    GetNetwork {
        source: icp::context::GetNetworkError,
    },

    #[snafu(transparent)]
    NetworkAccess { source: icp::network::AccessError },

    #[snafu(transparent)]
    AgentCreate {
        source: icp::agent::CreateAgentError,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForUrlError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    AgentCreate {
        source: icp::agent::CreateAgentError,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentError {
    #[snafu(transparent)]
    ProjectExists { source: ProjectLoadError },

    #[snafu(display("You can't specify both an environment and a network"))]
    EnvironmentAndNetworkSpecified,

    #[snafu(display(
        "No project found and no network specified. Either run this command inside a project or specify a network with --network"
    ))]
    NoProjectOrNetwork,

    #[snafu(transparent)]
    GetAgentForEnv { source: GetAgentForEnvError },

    #[snafu(transparent)]
    GetAgentForNetwork { source: GetAgentForNetworkError },

    #[snafu(transparent)]
    GetAgentForUrl { source: GetAgentForUrlError },
}

#[derive(Debug, Snafu)]
pub enum ContextInitError {
    #[snafu(display("failed to initialize directories"))]
    Directories {
        source: icp::directories::DirectoriesError,
    },

    #[snafu(display("failed to get current working directory"))]
    Cwd { source: std::io::Error },

    #[snafu(display("failed to convert path to UTF-8"))]
    Utf8Path { source: FromPathBufError },

    #[snafu(display("failed to lock package cache directory"))]
    PackageCache { source: icp::fs::lock::LockError },

    #[snafu(display("failed to lock identity directory"))]
    IdentityDirectory { source: icp::fs::lock::LockError },
}

/// Builds the context for this CLI invocation.
pub fn initialize(
    project_root_override: Option<PathBuf>,
    debug: bool,
    password_func: PasswordFunc,
    pem_session_duration: Option<Duration>,
) -> Result<Context, ContextInitError> {
    // Setup global directory structure
    let dirs = Arc::new(Directories::new().context(DirectoriesSnafu)?);

    // Project root locator
    let project_root_locate = Arc::new(ProjectRootLocateImpl::new(
        resolve_cwd()?,
        project_root_override,
    ));

    // Recipes
    let recipe = Arc::new(Handlebars {
        http_client: reqwest::Client::new(),
        pkg_cache: dirs.package_cache().context(PackageCacheSnafu)?,
    });

    // Project loader
    let project = Arc::new(Lazy::new(ProjectLoadImpl {
        project_root_locate: project_root_locate.clone(),
        recipe,
    }));

    let inner = icp::context::initialize(dirs.clone(), project_root_locate, project);

    // Identity loader
    let identity = Arc::new(crate::identity::Loader::new(
        IdentityPaths::new(dirs.identity_dir()).context(IdentityDirectorySnafu)?,
        password_func.clone(),
        pem_session_duration,
        inner.telemetry_data.clone(),
    ));
    if let Ok(mockdir) = std::env::var("ICP_CLI_KEYRING_MOCK_DIR") {
        keyring::set_default_credential_builder(Box::new(
            crate::identity::keyring_mock::MockKeyring {
                dir: PathBuf::from(mockdir),
            },
        ));
    }

    Ok(Context {
        inner,
        identity,
        debug,
        password_func,
    })
}

/// The directory to start looking for a project in.
///
/// On Unix, prefer $PWD (the logical path the user cd'd through) over
/// getcwd(3), which resolves symlinks to the physical path and would break
/// upward traversal when the user is inside a symlinked directory whose
/// manifest sits above the symlink's location.
///
/// Guard with an inode check: if $PWD was inherited from a parent process that
/// used chdir(2) without updating $PWD, the two paths point to different inodes
/// and we fall back to getcwd(). Because `metadata()` follows symlinks, a
/// symlinked $PWD still resolves to the same inode as getcwd(), so the symlink
/// case still works.
#[cfg(unix)]
fn resolve_cwd() -> Result<PathBuf, ContextInitError> {
    let real = PathBuf::try_from(current_dir().context(CwdSnafu)?).context(Utf8PathSnafu)?;
    Ok(std::env::var("PWD")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .filter(|p| same_inode(p.as_path(), real.as_path()))
        .unwrap_or(real))
}

#[cfg(not(unix))]
fn resolve_cwd() -> Result<PathBuf, ContextInitError> {
    PathBuf::try_from(current_dir().context(CwdSnafu)?).context(Utf8PathSnafu)
}

#[cfg(unix)]
fn same_inode(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev() && ma.ino() == mb.ino(),
        _ => false,
    }
}

#[cfg(test)]
impl Context {
    /// A context whose library ports are all mocks and whose identity loader
    /// serves the anonymous identity.
    pub fn mocked() -> Context {
        Context {
            inner: icp::context::Context::mocked(),
            identity: Arc::new(crate::identity::MockIdentityLoader::anonymous()),
            debug: false,
            password_func: Arc::new(|| Err("no password available in mock context".to_string())),
        }
    }
}

#[cfg(test)]
mod tests;
