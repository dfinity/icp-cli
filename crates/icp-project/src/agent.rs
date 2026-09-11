//! The agent an operation speaks through, without the means to make one.
//!
//! Building an agent takes an identity, a key store and a way to unlock it —
//! all of which belong to the surrounding application, not here. So this layer
//! names only what it needs: something it can ask for an agent when it has
//! something to say.

use std::{error::Error, future::Future};

use futures::future::BoxFuture;
use ic_agent::Agent;
use snafu::Snafu;
use tokio::sync::OnceCell;

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
                Box::pin(async move { creating.await.map_err(LazyAgentError::new) })
            }),
        }
    }

    /// The agent, creating it if this is the first call.
    pub async fn get(&self) -> Result<&Agent, LazyAgentError> {
        self.cell.get_or_try_init(|| (self.create)()).await
    }
}

/// An agent could not be created.
///
/// What that took is the caller's business: resolving an identity, unlocking a
/// key, reaching a network for its root key. This layer knows only that it can
/// fail and that whatever went wrong is what the user needs to be told, so the
/// cause is carried whole and displayed as itself rather than being restated
/// here.
#[derive(Debug, Snafu)]
#[snafu(transparent)]
pub struct LazyAgentError {
    pub source: Box<dyn Error + Send + Sync + 'static>,
}

impl LazyAgentError {
    /// Wraps a creation function's own error for the boundary.
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}
