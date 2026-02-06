use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use ic_agent::{Agent, AgentError, Identity};
use snafu::prelude::*;

use crate::prelude::*;

#[derive(Debug, Snafu)]
pub enum CreateAgentError {
    #[snafu(display("failed to create agent"))]
    Agent { source: AgentError },
}

#[async_trait]
pub trait Create: Sync + Send {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateAgentError>;
}

pub struct Creator;

#[async_trait]
impl Create for Creator {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateAgentError> {
        let mut b = Agent::builder().with_url(url).with_arc_identity(id);
        let default_ingress_expiry = Duration::from_secs(4 * MINUTE);
        if let Ok(ms) = std::env::var("ICP_CLI_TEST_ADVANCE_TIME_MS") {
            b = b.with_ingress_expiry(
                default_ingress_expiry
                    + Duration::from_millis(
                        ms.parse::<u64>()
                            .expect("ICP_CLI_TEST_ADVANCE_TIME_MS must be set to an int"),
                    ),
            );
        } else {
            b = b.with_ingress_expiry(default_ingress_expiry);
        }

        Ok(b.build().context(AgentSnafu)?)
    }
}
