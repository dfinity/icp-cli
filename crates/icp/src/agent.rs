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

/// How far ahead of now an agent dates the messages it expires, unless the
/// caller pins something else.
const DEFAULT_INGRESS_EXPIRY: Duration = Duration::from_secs(4 * MINUTE);

#[async_trait]
pub trait Create: Sync + Send {
    /// Builds an agent talking to `url` as `id`.
    ///
    /// `ingress_expiry` pins how far ahead of now the agent dates the messages it
    /// derives an expiry for. Pass `None` for the default. Pass `Some` only when
    /// the expiry is itself part of the output — signing a message here for
    /// another machine to submit, where the call envelope and the pre-signed
    /// `request_status` that accompanies it have to land in the same submission
    /// window. A pinned expiry is used verbatim, so the
    /// `ICP_CLI_TEST_ADVANCE_TIME_MS` clock offset applies to the default only.
    async fn create(
        &self,
        id: Arc<dyn Identity>,
        url: &str,
        ingress_expiry: Option<Duration>,
    ) -> Result<Agent, CreateAgentError>;
}

pub struct Creator;

#[async_trait]
impl Create for Creator {
    async fn create(
        &self,
        id: Arc<dyn Identity>,
        url: &str,
        ingress_expiry: Option<Duration>,
    ) -> Result<Agent, CreateAgentError> {
        let ingress_expiry =
            ingress_expiry.unwrap_or_else(|| DEFAULT_INGRESS_EXPIRY + test_time_advance());

        let b = Agent::builder()
            .with_url(url)
            .with_arc_identity(id)
            .with_ingress_expiry(ingress_expiry);

        Ok(b.build().context(AgentSnafu)?)
    }
}

/// How far a test has advanced the replica's clock past this machine's, so the
/// default ingress expiry stays ahead of replica time.
fn test_time_advance() -> Duration {
    match std::env::var("ICP_CLI_TEST_ADVANCE_TIME_MS") {
        Ok(ms) => Duration::from_millis(
            ms.parse::<u64>()
                .expect("ICP_CLI_TEST_ADVANCE_TIME_MS must be set to an int"),
        ),
        Err(_) => Duration::ZERO,
    }
}
