//! Getting hold of a wasm module the project points at but does not contain.

use camino::Utf8Path;
use icp_events::StepReporter;
use snafu::Snafu;

use crate::manifest::adapter::prebuilt::SourceField;
use crate::prelude::*;

/// A wasm module could not be produced.
///
/// A manifest may name a module by URL, so resolving one can mean an HTTP
/// request and a write to a cache outside the project. This layer knows only
/// that it can fail, so the cause is carried whole and displayed as itself.
#[derive(Debug, Snafu)]
#[snafu(transparent)]
pub struct FetchError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl FetchError {
    /// Wraps an implementation's own error for the trait boundary.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}

/// Where a build or sync step gets the wasm module it was told to use.
///
/// Asked for rather than done here: fetching over HTTP and writing to a cache
/// outside the project are not available to every caller of this crate.
#[async_trait::async_trait]
pub trait Fetch: Send + Sync {
    /// Resolve a wasm source to a local file, verifying `sha256` when one is
    /// given.
    async fn wasm(
        &self,
        source: &SourceField,
        base_dir: &Utf8Path,
        sha256: Option<&str>,
        reporter: &StepReporter,
    ) -> Result<PathBuf, FetchError>;
}

#[cfg(any(test, feature = "test-util"))]
/// A [`Fetch`] for tests on paths that never reach a wasm source.
pub struct UnimplementedMockFetch;

#[cfg(any(test, feature = "test-util"))]
#[async_trait::async_trait]
impl Fetch for UnimplementedMockFetch {
    async fn wasm(
        &self,
        _source: &SourceField,
        _base_dir: &Utf8Path,
        _sha256: Option<&str>,
        _reporter: &StepReporter,
    ) -> Result<PathBuf, FetchError> {
        unimplemented!("UnimplementedMockFetch::wasm")
    }
}
