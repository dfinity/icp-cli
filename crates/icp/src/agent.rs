use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use ic_agent::{Agent, AgentError, Identity};

use crate::prelude::*;

#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Create: Sync + Send {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateError>;
}

pub struct Creator;

#[async_trait]
impl Create for Creator {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateError> {
        let b = Agent::builder();

        // Url
        let b = b.with_url(url);

        // Identity
        let b = b.with_arc_identity(id);

        // Ingress Expiration
        let b = b.with_ingress_expiry(Duration::from_secs(4 * MINUTE));

        // // Key
        // if let Some(k) = todo!() {
        //     agent.set_root_key(k);
        // }

        Ok(b.build()?)
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
