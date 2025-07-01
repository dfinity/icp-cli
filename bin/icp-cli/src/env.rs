use crate::env::GetProjectError::ProjectNotFound;
use crate::options::NetworkOpt;
use crate::{store_artifact::ArtifactStore, store_id::IdStore};
use candid::Principal;
use ic_agent::{Agent, Identity};
use icp_dirs::IcpCliDirs;
use icp_identity::key::LoadIdentityInContextError;
use icp_network::access::{CreateAgentError, GetNetworkAccessError, NetworkAccess};
use icp_project::directory::{FindProjectError, ProjectDirectory};
use icp_project::project::{LoadProjectManifestError, NoSuchNetworkError, Project};
use snafu::Snafu;
use std::sync::{Arc, OnceLock};

pub struct Env {
    dirs: IcpCliDirs,
    identity_name: Option<String>,
    pub id_store: IdStore,
    pub artifact_store: ArtifactStore,
    project: TryOnceLock<Project>,
    identity: TryOnceLock<Arc<dyn Identity>>,
    network_name: OnceLock<String>,
    default_effective_canister_id: OnceLock<Principal>,
    agent: TryOnceLock<Agent>,
}

impl Env {
    pub fn new(
        dirs: IcpCliDirs,
        identity_name: Option<String>,
        id_store: IdStore,
        artifact_store: ArtifactStore,
    ) -> Self {
        Self {
            dirs,
            identity_name,
            id_store,
            artifact_store,
            project: TryOnceLock::new(),
            identity: TryOnceLock::new(),
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

    pub fn set_network_opt(&self, opt: NetworkOpt) {
        let network_name = opt.to_network_name();

        match self.network_name.get() {
            Some(existing) if *existing == network_name => {
                // Already set to the same value — fine, do nothing
            }
            Some(existing) => {
                // Already set to a different value — not allowed
                panic!(
                    "NetworkOpt was already set to a different value: {:?} vs {:?}",
                    existing, opt
                );
            }
            None => {
                // Not yet set — store it
                self.network_name
                    .set(network_name)
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

    pub fn agent(&self) -> Result<&Agent, EnvGetAgentError> {
        self.agent.get_or_try_init(|| self.create_agent())
    }
}

impl Env {
    fn load_identity(&self) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
        if let Some(identity) = &self.identity_name {
            Ok(icp_identity::key::load_identity(
                &self.dirs,
                &icp_identity::manifest::load_identity_list(&self.dirs)?,
                identity,
                || todo!(),
            )?)
        } else {
            icp_identity::key::load_identity_in_context(&self.dirs, || todo!())
        }
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

        if network_name == "ic" {
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

    fn create_agent(&self) -> Result<Agent, EnvGetAgentError> {
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
pub enum EnvGetAgentError {
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
