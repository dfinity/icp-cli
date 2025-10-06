use std::sync::{Arc, OnceLock};

use console::Term;
use ic_agent::{Agent, Identity};
use icp::Directories;
use icp_identity::{
    key::{LoadIdentityInContextError, load_identity, load_identity_in_context},
    manifest::load_identity_list,
};
use icp_network::{
    NETWORK_IC,
    access::{CreateAgentError, GetNetworkAccessError, NetworkAccess},
};
use snafu::Snafu;

use crate::{store_artifact::ArtifactStore, store_id::IdStore};

pub struct Context {
    /// Terminal for printing messages for the user to see
    pub term: Term,

    /// Canisters ID Store for lookup and storage
    pub id_store: IdStore,

    /// An artifact store for canister build artifacts
    pub artifact_store: ArtifactStore,

    dirs: Directories,

    /// The name of the identity to use, set from the command line.
    identity_name: OnceLock<Option<String>>,

    /// The network name, set from the command line for those commands that access a network.
    network_name: OnceLock<String>,

    /// Project loader
    pub project: Arc<dyn icp::Load>,

    /// The identity to use for the agent, instantiated on-demand.
    identity: TryOnceLock<Arc<dyn Identity>>,

    /// The agent used to access the network, instantiated on-demand.
    agent: TryOnceLock<Agent>,
}

impl Context {
    pub fn new(
        term: Term,
        dirs: Directories,
        id_store: IdStore,
        artifact_store: ArtifactStore,
        project: Arc<dyn icp::Load>,
    ) -> Self {
        Self {
            term,
            id_store,
            artifact_store,
            project,
            dirs,
            identity_name: OnceLock::new(),
            network_name: OnceLock::new(),
            identity: TryOnceLock::new(),
            agent: TryOnceLock::new(),
        }
    }

    pub fn dirs(&self) -> &Directories {
        &self.dirs
    }
}

impl Context {
    pub fn require_identity(&self, identity_name: Option<&str>) {
        match self.identity_name.get() {
            // Already set to the same value — fine, do nothing
            Some(existing) if existing.as_deref() == identity_name => {}

            // Already set to a different value — not allowed
            Some(existing) => panic!(
                "IdentityOpt was already set to a different value: {existing:?} vs {identity_name:?}"
            ),

            // Not yet set — store it
            None => self
                .identity_name
                .set(identity_name.map(|s| s.to_string()))
                .expect("Should only fail if already set"),
        }
    }

    pub fn identity(&self) -> Result<Arc<dyn Identity>, LoadIdentityInContextError> {
        self.identity
            .get_or_try_init(|| {
                let identity_name = self
                    .identity_name
                    .get()
                    .cloned()
                    .expect("identity has not been set");

                let id = match identity_name {
                    Some(name) => {
                        load_identity(
                            &self.dirs,                       // dirs
                            &load_identity_list(&self.dirs)?, // list
                            &name,                            // name
                            || todo!(),                       // password_func
                        )?
                    }

                    None => load_identity_in_context(
                        &self.dirs, // dirs
                        || todo!(), // password_func
                    )?,
                };

                Ok(id)
            })
            .cloned()
    }
}

#[derive(Debug, Snafu)]
pub enum CreateNetworkError {
    #[snafu(transparent)]
    GetNetworkAccess { source: GetNetworkAccessError },
}

impl Context {
    pub fn require_network(&self, network_name: &str) {
        match self.network_name.get() {
            // Already set to the same value — fine, do nothing
            Some(existing) if *existing == network_name => {}

            // Already set to a different value — not allowed
            Some(existing) => panic!(
                "NetworkOpt was already set to a different value: {existing} vs {network_name}"
            ),

            // Not yet set — store it
            None => self
                .network_name
                .set(network_name.to_string())
                .expect("Should only fail if already set"),
        }
    }

    async fn create_network_access(&self) -> Result<NetworkAccess, CreateNetworkError> {
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

        let ac = icp_network::access::get_network_access(
            //
            // nd
            project
                .directory
                .network(&network_name, self.dirs.port_descriptor()),
            //
            // config
            project.get_network_config(&network_name)?,
        )?;

        Ok(ac)
    }
}

#[derive(Debug, Snafu)]
pub enum ContextAgentError {
    #[snafu(transparent)]
    EnvGetNetworkAccess { source: CreateNetworkError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(transparent)]
    CreateAgent { source: CreateAgentError },
}

impl Context {
    pub fn agent(&self) -> Result<&Agent, ContextAgentError> {
        self.agent.get_or_try_init(|| {
            // Setup network
            let network_access = self.create_network_access()?;

            // Setup identity
            let identity = self.identity()?;

            // Setup agent
            let agent = network_access.create_agent(identity)?;

            Ok(agent)
        })
    }
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
