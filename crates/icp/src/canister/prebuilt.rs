use std::str::FromStr;

use async_trait::async_trait;
use reqwest::{Client, Method, Request};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;
use url::Url;

use crate::{
    canister::build::{Build, BuildError, Params, Step},
    fs::{read, write},
    manifest::adapter::prebuilt::SourceField,
};

// TODO(or.ricon): Put an http client in the struct
pub struct Prebuilt;

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
}

impl Prebuilt {
    async fn build_impl(
        &self,
        step: &Step,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), PrebuiltError> {
        let Step::Prebuilt(adapter) = step else {
            panic!("expected prebuilt adapter");
        };

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
            SourceField::Remote(s) => {
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

        // Verify the checksum if it's provided
        if let Some(expected) = &adapter.sha256 {
            if let Some(stdio) = &stdio {
                stdio
                    .send("Verifying checksum".to_string())
                    .await
                    .context(LogSnafu)?;
            }
            // Calculate checksum
            let actual = hex::encode({
                let mut h = Sha256::new();
                h.update(&wasm);
                h.finalize()
            });

            // Verify Checksum
            if &actual != expected {
                return ChecksumMismatchSnafu {
                    expected: expected.to_owned(),
                    actual,
                }
                .fail();
            }
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
}

#[async_trait]
impl Build for Prebuilt {
    async fn build(
        &self,
        step: &Step,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        Ok(self.build_impl(step, params, stdio).await?)
    }
}
