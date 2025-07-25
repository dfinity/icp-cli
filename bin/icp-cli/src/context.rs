use std::sync::{Arc, OnceLock};

use candid::Principal;
use ic_agent::{Agent, Identity};
use icp_dirs::IcpCliDirs;
use icp_identity::key::LoadIdentityInContextError;
use icp_network::{
    NETWORK_IC,
    access::{CreateAgentError, GetNetworkAccessError, NetworkAccess},
};
use icp_project::{
    directory::{FindProjectError, ProjectDirectory},
    project::{LoadProjectManifestError, NoSuchNetworkError, Project},
};
use snafu::Snafu;

use crate::context::GetProjectError::ProjectNotFound;
use crate::{store_artifact::ArtifactStore, store_id::IdStore};

pub struct Context {
    dirs: IcpCliDirs,

    /// The name of the identity to use, set from the command line.
    identity_name: OnceLock<Option<String>>,

    /// The identity to use for the agent, instantiated on-demand.
    identity: TryOnceLock<Arc<dyn Identity>>,

    pub id_store: IdStore,
    pub artifact_store: ArtifactStore,

    /// The current project, instantiated on-demand.
    project: TryOnceLock<Project>,

    /// The network name, set from the command line for those commands that access a network.
    network_name: OnceLock<String>,

    /// The default effective canister ID for the network, available for managed networks
    /// after creating the agent.
    default_effective_canister_id: OnceLock<Principal>,

    /// The agent used to access the network, instantiated on-demand.
    agent: TryOnceLock<Agent>,
}

impl Context {
    pub fn new(dirs: IcpCliDirs, id_store: IdStore, artifact_store: ArtifactStore) -> Self {
        Self {
            dirs,
            identity_name: OnceLock::new(),
            identity: TryOnceLock::new(),
            id_store,
            artifact_store,
            project: TryOnceLock::new(),
            network_name: OnceLock::new(),
            default_effective_canister_id: OnceLock::new(),
            agent: TryOnceLock::new(),
        }
    }

    pub fn dirs(&self) -> &IcpCliDirs {
        &self.dirs
    }

    // Accessors using TryOnceLock only cache success.
    // Errors are re-evaluated on each access attempt.

    pub fn identity(&self) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
        self.identity
            .get_or_try_init(|| self.load_identity())
            .cloned()
    }

    pub fn project(&self) -> Result<&Project, GetProjectError> {
        self.project
            .get_or_try_init(|| self.find_and_load_project())
    }

    pub fn require_identity(&self, identity_name: Option<&str>) {
        match self.identity_name.get() {
            Some(existing) if existing.as_deref() == identity_name => {
                // Already set to the same value — fine, do nothing
            }
            Some(existing) => {
                // Already set to a different value — not allowed
                panic!(
                    "IdentityOpt was already set to a different value: {existing:?} vs {identity_name:?}"
                );
            }
            None => {
                // Not yet set — store it
                self.identity_name
                    .set(identity_name.map(|s| s.to_string()))
                    .expect("Should only fail if already set");
            }
        }
    }

    pub fn require_network(&self, network_name: &str) {
        match self.network_name.get() {
            Some(existing) if *existing == network_name => {
                // Already set to the same value — fine, do nothing
            }
            Some(existing) => {
                // Already set to a different value — not allowed
                panic!(
                    "NetworkOpt was already set to a different value: {existing} vs {network_name}"
                );
            }
            None => {
                // Not yet set — store it
                self.network_name
                    .set(network_name.to_string())
                    .expect("Should only fail if already set");
            }
        }
    }

    // The default effective canister ID is available for local networks
    // after constructing the agent.
    #[allow(dead_code)]
    pub fn default_effective_canister_id(
        &self,
    ) -> Result<Principal, NoDefaultEffectiveCanisterIdError> {
        self.default_effective_canister_id
            .get()
            .ok_or(NoDefaultEffectiveCanisterIdError)
            .cloned()
    }

    pub fn agent(&self) -> Result<&Agent, ContextGetAgentError> {
        self.agent.get_or_try_init(|| self.create_agent())
    }
}

impl Context {
    fn load_identity(&self) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
        let identity_name = self
            .identity_name
            .get()
            .cloned()
            .expect("identity has not been set");

        if let Some(identity) = identity_name {
            return Ok(icp_identity::key::load_identity(
                &self.dirs,
                &icp_identity::manifest::load_identity_list(&self.dirs)?,
                &identity,
                || todo!(),
            )?);
        }

        icp_identity::key::load_identity_in_context(&self.dirs, || todo!())
    }

    fn find_and_load_project(&self) -> Result<Project, GetProjectError> {
        let pd = ProjectDirectory::find()?.ok_or(ProjectNotFound)?;

        let project = Project::load(pd)?;
        Ok(project)
    }

    fn create_network_access(&self) -> Result<NetworkAccess, EnvGetNetworkAccessError> {
        let network_name = self
            .network_name
            .get()
            .cloned()
            .expect("call set_network_opt before get_network_access");

        if network_name == NETWORK_IC {
            return Ok(NetworkAccess::mainnet());
        }

        // For other networks, we need to load the project
        // in order to read the network configuration.
        let project = self.project()?;
        let nd = project
            .directory
            .network(&network_name, self.dirs.port_descriptor_dir());
        let network_config = project.get_network_config(&network_name)?;

        let ac = icp_network::access::get_network_access(nd, network_config)?;

        if let Some(default_effective_canister_id) = ac.default_effective_canister_id {
            self.default_effective_canister_id
                .set(default_effective_canister_id)
                .expect("default effective canister id should only be set once");
        }

        Ok(ac)
    }

    fn create_agent(&self) -> Result<Agent, ContextGetAgentError> {
        let network_access = self.create_network_access()?;
        let identity = self.identity()?;
        let agent = network_access.create_agent(identity)?;
        Ok(agent)
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("no default effective canister id set"))]
pub struct NoDefaultEffectiveCanisterIdError;

#[derive(Debug, Snafu)]
pub enum EnvGetNetworkAccessError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    GetNetworkAccess { source: GetNetworkAccessError },

    #[snafu(transparent)]
    NoSuchNetwork { source: NoSuchNetworkError },
}

#[derive(Debug, Snafu)]
pub enum ContextGetAgentError {
    #[snafu(transparent)]
    EnvGetNetworkAccess { source: EnvGetNetworkAccessError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(transparent)]
    CreateAgent { source: CreateAgentError },
}

#[derive(Debug, Snafu)]
pub enum GetProjectError {
    #[snafu(transparent)]
    FindProjectDirectory { source: FindProjectError },

    #[snafu(transparent)]
    LoadProjectManifest { source: LoadProjectManifestError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,
}

#[derive(Debug)]
pub struct TryOnceLock<T> {
    inner: OnceLock<T>,
}

// todo(ericswanson): when OnceLock::get_or_try_init is stabilized, use that instead
impl<T> TryOnceLock<T> {
    pub fn new() -> Self {
        Self {
            inner: OnceLock::new(),
        }
    }

    pub fn get_or_try_init<E, F>(&self, f: F) -> Result<&T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let Some(val) = self.inner.get() {
            Ok(val)
        } else {
            let value = f()?;
            Ok(self.inner.get_or_init(|| value))
        }
    }
}
