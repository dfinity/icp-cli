use camino::{Utf8Path, Utf8PathBuf};
use reqwest::{Client, Method, Request};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;
use url::Url;

use crate::{
    fs::read,
    manifest::adapter::prebuilt::SourceField,
    package::{PackageCache, cache_wasm, read_cached_wasm},
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

    #[snafu(display("failed to send log message"))]
    Log {
        source: tokio::sync::mpsc::error::SendError<String>,
    },

    #[snafu(display("failed to read cached wasm file"))]
    ReadCache { source: crate::fs::IoError },

    #[snafu(display("failed to cache wasm file"))]
    CacheFile { source: crate::fs::IoError },

    #[snafu(display("failed to acquire lock on package cache"))]
    LockCache { source: crate::fs::lock::LockError },
}

/// Fetch wasm bytes from a `SourceField` (local path or remote URL), optionally verifying
/// the sha256 checksum. Does not interact with the cache.
async fn fetch(
    source: &SourceField,
    base_dir: &Utf8Path,
    sha256: Option<&str>,
    stdio: Option<&Sender<String>>,
) -> Result<Vec<u8>, WasmError> {
    let bytes = match source {
        SourceField::Local(s) => {
            let path = base_dir.join(&s.path);
            if let Some(tx) = stdio {
                tx.send(format!("Reading wasm: {path}"))
                    .await
                    .context(LogSnafu)?;
            }
            read(&path).context(ReadLocalSnafu { path })?
        }
        SourceField::Remote(s) => {
            let url = Url::parse(&s.url).context(ParseUrlSnafu)?;
            if let Some(tx) = stdio {
                tx.send(format!("Fetching wasm: {url}"))
                    .await
                    .context(LogSnafu)?;
            }
            let resp = Client::new()
                .execute(Request::new(Method::GET, url))
                .await
                .context(HttpRequestSnafu)?;
            let status = resp.status();
            if !status.is_success() {
                return HttpStatusSnafu { status }.fail();
            }
            resp.bytes().await.context(HttpResponseSnafu)?.to_vec()
        }
    };

    if let Some(expected) = sha256 {
        if let Some(tx) = stdio {
            tx.send("Verifying checksum".to_string())
                .await
                .context(LogSnafu)?;
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

    Ok(bytes)
}

/// Resolve wasm bytes from a `SourceField` (local path or remote URL), optionally verifying
/// the sha256 checksum. For remote sources, checks the local cache before downloading and
/// stores the result afterwards.
pub async fn resolve(
    source: &SourceField,
    base_dir: &Utf8Path,
    sha256: Option<&str>,
    stdio: Option<&Sender<String>>,
    pkg_cache: &PackageCache,
) -> Result<Vec<u8>, WasmError> {
    if let (SourceField::Remote(_), Some(expected)) = (source, sha256) {
        let maybe_cached = pkg_cache
            .with_read(async |r| read_cached_wasm(r, expected).context(ReadCacheSnafu))
            .await
            .context(LockCacheSnafu)?;
        if let Some(cached) = maybe_cached? {
            if let Some(tx) = stdio {
                tx.send("Using cached file".to_string())
                    .await
                    .context(LogSnafu)?;
            }
            return Ok(cached);
        }
    }

    let bytes = fetch(source, base_dir, sha256, stdio).await?;

    if matches!(source, SourceField::Remote(_)) {
        let cksum = hex::encode(Sha256::digest(&bytes));
        pkg_cache
            .with_write(async |w| cache_wasm(w, &cksum, &bytes).context(CacheFileSnafu))
            .await
            .context(LockCacheSnafu)??;
    }

    Ok(bytes)
}

/// Returns the stable on-disk path for a cached wasm by sha256.
pub async fn cached_path(
    pkg_cache: &PackageCache,
    sha: &str,
) -> Result<crate::prelude::PathBuf, crate::fs::lock::LockError> {
    pkg_cache.with_read(async |r| r.wasm_sha(sha).wasm()).await
}
