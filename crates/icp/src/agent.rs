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

        Ok(b.build()?)
    }
}

// ============================================================================
// Test utilities
// ============================================================================

#[cfg(any(test, feature = "test-utils"))]
pub mod test {
    use std::sync::Arc;

    use async_trait::async_trait;
    use ic_agent::{Agent, Identity};

    use super::*;

    /// Mock agent creator for testing.
    ///
    /// Note: Agent cannot be easily cloned, so this mock always returns an error.
    /// For testing purposes, you typically don't need a real Agent in unit tests.
    pub struct MockAgentCreator {
        result: Result<Agent, String>,
    }

    impl MockAgentCreator {
        pub fn new(agent: Agent) -> Self {
            Self { result: Ok(agent) }
        }

        pub fn with_error(msg: impl Into<String>) -> Self {
            Self {
                result: Err(msg.into()),
            }
        }

        pub fn dummy_agent() -> Agent {
            Agent::builder()
                .with_url("http://localhost:8000")
                .build()
                .unwrap()
        }
    }

    impl Default for MockAgentCreator {
        fn default() -> Self {
            Self::new(Self::dummy_agent())
        }
    }

    #[async_trait]
    impl Create for MockAgentCreator {
        async fn create(&self, _id: Arc<dyn Identity>, _url: &str) -> Result<Agent, CreateError> {
            match &self.result {
                Ok(a) => Ok(a.clone()),
                Err(msg) => Err(CreateError::Unexpected(anyhow::anyhow!("{}", msg))),
            }
        }
    }
}
