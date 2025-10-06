use std::sync::Arc;

use console::Term;
use icp::{Directories, identity};

use crate::{store_artifact::ArtifactStore, store_id::IdStore};

pub struct Context {
    /// Various cli-related directories (cache, configuration, etc).
    pub dirs: Directories,

    /// Terminal for printing messages for the user to see
    pub term: Term,

    /// Canisters ID Store for lookup and storage
    pub id_store: IdStore,

    /// An artifact store for canister build artifacts
    pub artifact_store: ArtifactStore,

    /// Project loader
    pub project: Arc<dyn icp::Load>,

    /// Identity loader
    pub identity: Arc<dyn identity::Load>,
}

impl Context {
    pub fn new(
        term: Term,
        dirs: Directories,
        id_store: IdStore,
        artifact_store: ArtifactStore,
        project: Arc<dyn icp::Load>,
        identity: Arc<dyn identity::Load>,
    ) -> Self {
        Self {
            dirs,

            // Display
            term,

            // Storage
            id_store,
            artifact_store,

            // Loaders
            project,
            identity,
        }
    }
}

// impl Context {
//     async fn create_network_access(&self, name: &str) -> Result<NetworkAccess, CreateNetworkError> {
//         if name == NETWORK_IC {
//             return Ok(NetworkAccess::mainnet());
//         }

//         // For other networks, we need to load the project
//         // in order to read the network configuration.
//         let project = self.project.load().await?;

//         let ac = icp_network::access::get_network_access(
//             //
//             // nd
//             project
//                 .directory
//                 .network(&name, self.dirs.port_descriptor()),
//             //
//             // config
//             project.get_network_config(&name)?,
//         )?;

//         Ok(ac)
//     }
// }

// impl Context {
//     pub async fn agent(
//         &self,
//         network: &str,
//         identity: Option<String>,
//     ) -> Result<Agent, ContextAgentError> {
//         // Setup network
//         let network_access = self.create_network_access(network).await?;

//         // Setup identity
//         let identity = self.identity(identity)?;

//         // Setup agent
//         let agent = network_access.create_agent(identity)?;

//         Ok(agent)
//     }
// }
