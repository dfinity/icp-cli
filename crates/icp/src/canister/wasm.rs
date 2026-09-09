use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use icp_events::StepReporter;
use reqwest::{Client, Method, Request};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use url::Url;

use crate::{
    fs::read,
    manifest::adapter::prebuilt::SourceField,
    package::{PackageCache, cache_wasm},
};

#[derive(Debug, Snafu)]
pub enum WasmError {
    #[snafu(display("failed to read wasm file at '{path}'"))]
    ReadLocal {
        source: crate::fs::IoError,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to parse wasm url"))]
    ParseUrl { source: url::ParseError },

    #[snafu(display("failed to fetch wasm file"))]
    HttpRequest { source: reqwest::Error },

    #[snafu(display("http request failed: {status}"))]
    HttpStatus { status: reqwest::StatusCode },

    #[snafu(display("failed to read http response"))]
    HttpResponse { source: reqwest::Error },

    #[snafu(display("checksum mismatch, expected: {expected}, actual: {actual}"))]
    ChecksumMismatch { expected: String, actual: String },

    #[snafu(display("failed to cache wasm file"))]
    CacheFile { source: crate::fs::IoError },

    #[snafu(display("failed to acquire lock on package cache"))]
    LockCache { source: crate::fs::lock::LockError },
}

/// Getting hold of a wasm module the project points at but does not contain.
///
/// A manifest may name a module by URL, so resolving one can mean an HTTP
/// request and a write to a cache that lives outside the project — neither of
/// which every caller of this crate can do. So it is asked for rather than
/// done here.
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
    ) -> Result<crate::prelude::PathBuf, WasmError>;
}

/// The [`Fetch`] that downloads over HTTP and caches in the package cache.
pub struct Fetcher {
    http_client: Client,
    pkg_cache: Arc<PackageCache>,
}

impl Fetcher {
    pub fn new(http_client: Client, pkg_cache: Arc<PackageCache>) -> Self {
        Self {
            http_client,
            pkg_cache,
        }
    }
}

#[async_trait::async_trait]
impl Fetch for Fetcher {
    /// - Local: verifies sha256 if provided, returns the local path.
    /// - Remote with sha256: checks the cache first; downloads, verifies, and caches on miss.
    /// - Remote without sha256: always downloads, computes sha256, caches by the computed sha256.
    async fn wasm(
        &self,
        source: &SourceField,
        base_dir: &Utf8Path,
        sha256: Option<&str>,
        reporter: &StepReporter,
    ) -> Result<crate::prelude::PathBuf, WasmError> {
        let pkg_cache = &self.pkg_cache;
        match source {
            SourceField::Local(s) => {
                let path = base_dir.join(&s.path);
                if let Some(expected) = sha256 {
                    reporter.info(format!("Reading wasm: {}", s.path));
                    let bytes = read(&path).context(ReadLocalSnafu {
                        path: s.path.clone(),
                    })?;
                    reporter.info("Verifying checksum");
                    let actual = hex::encode(Sha256::digest(&bytes));
                    ensure!(
                        actual == expected,
                        ChecksumMismatchSnafu {
                            expected: expected.to_owned(),
                            actual,
                        }
                    );
                }
                Ok(path)
            }
            SourceField::Remote(s) => {
                // Pre-download cache check is only possible when sha256 is known.
                if let Some(expected) = sha256 {
                    let cached = pkg_cache
                        .with_read(async |r| {
                            let wasm_cache = r.wasm_sha(expected);
                            let path = wasm_cache.wasm();
                            if path.exists() {
                                _ = crate::fs::write(&wasm_cache.atime(), b"");
                                Some(path)
                            } else {
                                None
                            }
                        })
                        .await
                        .context(LockCacheSnafu)?;
                    if let Some(path) = cached {
                        reporter.info("Using cached file");
                        return Ok(path);
                    }
                }

                let url = Url::parse(&s.url).context(ParseUrlSnafu)?;
                reporter.info(format!("Fetching wasm: {url}"));
                let resp = self
                    .http_client
                    .execute(Request::new(Method::GET, url))
                    .await
                    .context(HttpRequestSnafu)?;
                let status = resp.status();
                if !status.is_success() {
                    return HttpStatusSnafu { status }.fail();
                }
                let bytes = resp.bytes().await.context(HttpResponseSnafu)?.to_vec();

                // Use provided sha256 as cache key (after verifying), or compute from bytes.
                let cache_sha = match sha256 {
                    Some(expected) => {
                        reporter.info("Verifying checksum");
                        let actual = hex::encode(Sha256::digest(&bytes));
                        ensure!(
                            actual == expected,
                            ChecksumMismatchSnafu {
                                expected: expected.to_owned(),
                                actual,
                            }
                        );
                        actual
                    }
                    None => hex::encode(Sha256::digest(&bytes)),
                };

                pkg_cache
                    .with_write(async |w| cache_wasm(w, &cache_sha, &bytes).context(CacheFileSnafu))
                    .await
                    .context(LockCacheSnafu)??;

                pkg_cache
                    .with_read(async |r| r.wasm_sha(&cache_sha).wasm())
                    .await
                    .context(LockCacheSnafu)
            }
        }
    }
}

#[cfg(test)]
/// A [`Fetch`] for tests on paths that never reach a wasm source.
pub struct UnimplementedMockFetch;

#[cfg(test)]
#[async_trait::async_trait]
impl Fetch for UnimplementedMockFetch {
    async fn wasm(
        &self,
        _source: &SourceField,
        _base_dir: &Utf8Path,
        _sha256: Option<&str>,
        _reporter: &StepReporter,
    ) -> Result<crate::prelude::PathBuf, WasmError> {
        unimplemented!("UnimplementedMockFetch::wasm")
    }
}
