use std::str::FromStr;

use reqwest::{Client, Method, Request};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;
use url::Url;

use crate::{
    fs::{read, write},
    manifest::adapter::prebuilt::{Adapter, SourceField},
    package::{PackageCache, cache_prebuilt, read_cached_prebuilt},
};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PrebuiltError {
    #[snafu(display("failed to send log message"))]
    Log {
        source: tokio::sync::mpsc::error::SendError<String>,
    },

    #[snafu(display("failed to read prebuilt canister file"))]
    ReadFile { source: crate::fs::IoError },

    #[snafu(display("failed to parse prebuilt canister url"))]
    ParseUrl { source: url::ParseError },

    #[snafu(display("failed to fetch prebuilt canister file"))]
    HttpRequest { source: reqwest::Error },

    #[snafu(display("http request failed: {status}"))]
    HttpStatus { status: reqwest::StatusCode },

    #[snafu(display("failed to read http response"))]
    HttpResponse { source: reqwest::Error },

    #[snafu(display("checksum mismatch, expected: {expected}, actual: {actual}"))]
    ChecksumMismatch { expected: String, actual: String },

    #[snafu(display("failed to write wasm output file"))]
    WriteFile { source: crate::fs::IoError },

    #[snafu(display("failed to read cached prebuilt canister file"))]
    ReadCache { source: crate::fs::IoError },

    #[snafu(display("failed to cache wasm file"))]
    CacheFile { source: crate::fs::IoError },

    #[snafu(display("failed to acquire lock on package cache"))]
    LockCache { source: crate::fs::lock::LockError },
}

pub(super) async fn build(
    adapter: &Adapter,
    params: &Params,
    stdio: Option<Sender<String>>,
    pkg_cache: &PackageCache,
) -> Result<(), PrebuiltError> {
    let wasm = match &adapter.source {
        // Local path
        SourceField::Local(s) => {
            if let Some(stdio) = &stdio {
                stdio
                    .send(format!("Reading local file: {}", s.path))
                    .await
                    .context(LogSnafu)?;
            }
            read(&params.path.join(&s.path)).context(ReadFileSnafu)?
        }

        // Remote url
        SourceField::Remote(s) => 'wasm: {
            // If it's already cached, use it instead of downloading again
            if let Some(expected) = &adapter.sha256 {
                let maybe_cached = pkg_cache
                    .with_read(async |r| read_cached_prebuilt(r, expected).context(ReadCacheSnafu))
                    .await
                    .context(LockCacheSnafu)?;
                if let Some(cached) = maybe_cached? {
                    if let Some(stdio) = &stdio {
                        stdio
                            .send("Using cached file".to_string())
                            .await
                            .context(LogSnafu)?;
                    }
                    break 'wasm cached;
                }
            }
            // Initialize a new http client
            let http_client = Client::new();

            // Parse Url
            let u = Url::from_str(&s.url).context(ParseUrlSnafu)?;
            if let Some(stdio) = &stdio {
                stdio
                    .send(format!("Fetching remote file: {}", u))
                    .await
                    .context(LogSnafu)?;
            }

            // Construct request
            let req = Request::new(
                Method::GET,  // method
                u.to_owned(), // url
            );

            // Execute request
            let resp = http_client.execute(req).await.context(HttpRequestSnafu)?;

            let status = resp.status();

            // Check for success
            if !status.is_success() {
                return HttpStatusSnafu { status }.fail();
            }

            // Read response body
            resp.bytes().await.context(HttpResponseSnafu)?.to_vec()
        }
    };

    // Calculate checksum
    let cksum = hex::encode({
        let mut h = Sha256::new();
        h.update(&wasm);
        h.finalize()
    });

    // Verify the checksum if it's provided
    if let Some(expected) = &adapter.sha256 {
        if let Some(stdio) = &stdio {
            stdio
                .send("Verifying checksum".to_string())
                .await
                .context(LogSnafu)?;
        }

        // Verify Checksum
        if &cksum != expected {
            return ChecksumMismatchSnafu {
                expected: expected.to_owned(),
                actual: cksum,
            }
            .fail();
        }
    }

    if matches!(&adapter.source, SourceField::Remote(_)) {
        // Cache to disk
        pkg_cache
            .with_write(async |w| cache_prebuilt(w, &cksum, &wasm).context(CacheFileSnafu))
            .await
            .context(LockCacheSnafu)??;
    }

    // Set WASM file
    if let Some(stdio) = stdio {
        stdio
            .send(format!("Writing WASM file: {}", params.output))
            .await
            .context(LogSnafu)?;
    }
    write(
        &params.output, // path
        &wasm,          // contents
    )
    .context(WriteFileSnafu)?;

    Ok(())
}
