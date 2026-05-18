use camino::{Utf8Path, Utf8PathBuf};
use reqwest::{Client, Method, Request};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;
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

/// Resolve a wasm source to a local filesystem path, optionally verifying the sha256 checksum.
///
/// - Local: verifies sha256 if provided, returns the local path.
/// - Remote with sha256: checks the cache first; downloads, verifies, and caches on miss.
/// - Remote without sha256: always downloads, computes sha256, caches by the computed sha256.
pub async fn resolve(
    source: &SourceField,
    base_dir: &Utf8Path,
    sha256: Option<&str>,
    stdio: Option<&Sender<String>>,
    pkg_cache: &PackageCache,
) -> Result<crate::prelude::PathBuf, WasmError> {
    match source {
        SourceField::Local(s) => {
            let path = base_dir.join(&s.path);
            if let Some(expected) = sha256 {
                if let Some(tx) = stdio {
                    let _ = tx.send(format!("Reading wasm: {}", s.path)).await;
                }
                let bytes = read(&path).context(ReadLocalSnafu {
                    path: s.path.clone(),
                })?;
                if let Some(tx) = stdio {
                    let _ = tx.send("Verifying checksum".to_string()).await;
                }
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
                    if let Some(tx) = stdio {
                        let _ = tx.send("Using cached file".to_string()).await;
                    }
                    return Ok(path);
                }
            }

            let url = Url::parse(&s.url).context(ParseUrlSnafu)?;
            if let Some(tx) = stdio {
                let _ = tx.send(format!("Fetching wasm: {url}")).await;
            }
            let resp = Client::new()
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
                    if let Some(tx) = stdio {
                        let _ = tx.send("Verifying checksum".to_string()).await;
                    }
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
