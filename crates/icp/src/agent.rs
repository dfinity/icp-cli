use std::{error::Error, fmt, future::Future, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::future::BoxFuture;
use ic_agent::{Agent, AgentError, Identity};
use snafu::prelude::*;
use tokio::sync::OnceCell;

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

/// An [`Agent`] created on first use.
///
/// Creating an agent unlocks an identity — a password prompt, for an encrypted
/// one — and, on a network whose root key is fetched, costs a round trip. An
/// operation that can fail before it ever speaks to the network, such as a
/// deploy whose build fails, should cost neither: it takes one of these and the
/// first phase that actually needs the network pays for it.
pub struct LazyAgent<'a> {
    cell: OnceCell<Agent>,
    create: Box<dyn Fn() -> BoxFuture<'a, Result<Agent, LazyAgentError>> + Send + Sync + 'a>,
}

impl<'a> LazyAgent<'a> {
    /// Wraps whatever the caller does to produce an agent.
    ///
    /// `create` runs on the first [`get`](Self::get), and on a later one only
    /// while it keeps failing; the first agent it yields is the one every
    /// caller sees.
    pub fn new<F, Fut, E>(create: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'a,
        Fut: Future<Output = Result<Agent, E>> + Send + 'a,
        E: Error + Send + Sync + 'static,
    {
        Self {
            cell: OnceCell::new(),
            create: Box::new(move || {
                let creating = create();
                Box::pin(async move { creating.await.map_err(|e| LazyAgentError(Box::new(e))) })
            }),
        }
    }

    /// The agent, creating it if this is the first call.
    pub async fn get(&self) -> Result<&Agent, LazyAgentError> {
        self.cell.get_or_try_init(|| (self.create)()).await
    }
}

/// Whatever went wrong in a [`LazyAgent`]'s creation function.
///
/// Type-erased, and hand-written rather than a Snafu variant, because how an
/// identity is resolved and unlocked belongs to the caller: an operation holding
/// a `LazyAgent` cannot name that error, and does nothing with it but report it.
/// So this adds no message of its own — display and source both pass straight
/// through, as `snafu(transparent)` would.
#[derive(Debug)]
pub struct LazyAgentError(Box<dyn Error + Send + Sync>);

impl fmt::Display for LazyAgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Error for LazyAgentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.0.source()
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
