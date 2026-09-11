//! Choosing at random.
//!
//! Only one thing here needs randomness — picking which subnet to create a
//! canister on when the manifest names a kind rather than a subnet — and it
//! cannot be done by arithmetic. On a host it is a syscall; inside a canister
//! it is a `raw_rand` call to the management canister, which is why this is
//! asked for rather than done.

use async_trait::async_trait;
use snafu::Snafu;

/// Randomness could not be obtained.
///
/// Where it comes from is the implementation's business — a syscall, a
/// management-canister call — so the cause is carried whole and displayed as
/// itself.
#[derive(Debug, Snafu)]
#[snafu(transparent)]
pub struct RandomError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl RandomError {
    /// Wraps an implementation's own error for the trait boundary.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}

/// A source of randomness.
///
/// The method is a choice rather than a byte buffer because a choice is what
/// every caller wants, and turning bytes into an unbiased index is the kind of
/// thing each caller would get subtly wrong on its own.
#[async_trait]
pub trait Random: Send + Sync {
    /// A uniformly distributed index below `count`, or `None` when `count` is
    /// zero and there is nothing to choose.
    async fn index_below(&self, count: usize) -> Result<Option<usize>, RandomError>;
}

#[cfg(feature = "host")]
/// [`Random`] from the operating system's entropy.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostRandom;

#[cfg(feature = "host")]
#[async_trait]
impl Random for HostRandom {
    async fn index_below(&self, count: usize) -> Result<Option<usize>, RandomError> {
        use rand::RngExt;
        match count {
            0 => Ok(None),
            count => Ok(Some(rand::rng().random_range(0..count))),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
/// [`Random`] that always chooses the first of anything, for a test that needs
/// a choice made but not an unpredictable one.
#[derive(Debug, Default, Clone, Copy)]
pub struct FirstChoice;

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl Random for FirstChoice {
    async fn index_below(&self, count: usize) -> Result<Option<usize>, RandomError> {
        Ok((count > 0).then_some(0))
    }
}
