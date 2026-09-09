//! Something the caller can produce, produced on first use.
//!
//! What it takes to reach a canister — an identity, a key store and a way to
//! unlock it, an endpoint, a root key — belongs to the surrounding application,
//! not here. So this layer names only what it needs: something it can ask when
//! it has something to say.

use std::{error::Error, future::Future};

use futures::future::BoxFuture;
use snafu::Snafu;
use tokio::sync::OnceCell;

/// A `T` created on first use.
///
/// Anything that speaks for an identity costs something to make: unlocking a
/// key, which can mean a password prompt, and on a network whose root key is
/// fetched, a round trip. An operation that can fail before it ever speaks to
/// the network, such as a deploy whose build fails, should cost neither: it
/// takes one of these and the first phase that actually needs the network pays
/// for it.
pub struct Deferred<'a, T> {
    cell: OnceCell<T>,
    create: Box<dyn Fn() -> BoxFuture<'a, Result<T, DeferredError>> + Send + Sync + 'a>,
}

impl<'a, T> Deferred<'a, T> {
    /// Wraps whatever the caller does to produce the value.
    ///
    /// `create` runs on the first [`get`](Self::get), and on a later one only
    /// while it keeps failing; the first value it yields is the one every caller
    /// sees.
    pub fn new<F, Fut, E>(create: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'a,
        Fut: Future<Output = Result<T, E>> + Send + 'a,
        E: Error + Send + Sync + 'static,
    {
        Self {
            cell: OnceCell::new(),
            create: Box::new(move || {
                let creating = create();
                Box::pin(async move { creating.await.map_err(DeferredError::new) })
            }),
        }
    }

    /// The value, creating it if this is the first call.
    pub async fn get(&self) -> Result<&T, DeferredError> {
        self.cell.get_or_try_init(|| (self.create)()).await
    }
}

/// A [`Deferred`] value could not be created.
///
/// What that took is the caller's business: resolving an identity, unlocking a
/// key, reaching a network for its root key. This layer knows only that it can
/// fail and that whatever went wrong is what the user needs to be told, so the
/// cause is carried whole and displayed as itself rather than being restated
/// here.
#[derive(Debug, Snafu)]
#[snafu(display("{source}"))]
pub struct DeferredError {
    pub source: Box<dyn Error + Send + Sync + 'static>,
}

impl DeferredError {
    /// Wraps a creation function's own error for the boundary.
    pub fn new(source: impl Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}
