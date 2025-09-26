use std::{fmt, str::FromStr};

use crate::build::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use icp::{
    fs::{read, write},
    prelude::*,
};
use reqwest::{Client, Method, Request, StatusCode, Url};
use schemars::JsonSchema;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use snafu::Snafu;

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct LocalSource {
    /// Local path on-disk to read a WASM file from
    #[schemars(with = "String")]
    pub path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct RemoteSource {
    /// Url to fetch the remote WASM file from
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged, rename_all = "lowercase")]
pub enum SourceField {
    /// Local path on-disk to read a WASM file from
    Local(LocalSource),

    /// Remote url to fetch a WASM file from
    Remote(RemoteSource),
}

/// Configuration for a Pre-built canister build adapter.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct PrebuiltAdapter {
    #[serde(flatten)]
    pub source: SourceField,

    /// Optional sha256 checksum of the WASM
    pub sha256: Option<String>,
}

impl fmt::Display for PrebuiltAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let src = match &self.source {
            SourceField::Local(v) => format!("path: {}", v.path),
            SourceField::Remote(v) => format!("url: {}", v.url),
        };

        let sha = match &self.sha256 {
            Some(v) => v,
            None => "n/a",
        };

        write!(f, "({src}, sha: {sha})")
    }
}

#[async_trait]
impl Adapter for PrebuiltAdapter {
    async fn compile(
        &self,
        canister_path: &Path,
        wasm_output_path: &Path,
    ) -> Result<(), AdapterCompileError> {
        let wasm = match &self.source {
            // Local path
            SourceField::Local(s) => read(&canister_path.join(&s.path))
                .map_err(|err| PrebuiltAdapterCompileError::ReadFile { source: err })?,

            // Remote url
            SourceField::Remote(s) => {
                // Initialize a new http client
                let http_client = Client::new();

                // Parse Url
                let u = Url::from_str(&s.url)
                    .map_err(|err| PrebuiltAdapterCompileError::Url { source: err })?;

                // Construct request
                let req = Request::new(
                    Method::GET,  // method
                    u.to_owned(), // url
                );

                // Execute request
                let resp = http_client
                    .execute(req)
                    .await
                    .map_err(|err| PrebuiltAdapterCompileError::Request { source: err })?;

                let status = resp.status();

                // Check for success
                if !status.is_success() {
                    return Err(PrebuiltAdapterCompileError::Status {
                        url: u,
                        code: status,
                    }
                    .into());
                }

                // Read response body
                resp.bytes()
                    .await
                    .map_err(|err| PrebuiltAdapterCompileError::Request { source: err })?
                    .to_vec()
            }
        };

        // Verify the checksum if it's provided
        if let Some(expected) = &self.sha256 {
            // Calculate checksum
            let actual = hex::encode({
                let mut h = Sha256::new();
                h.update(&wasm);
                h.finalize()
            });

            // Verify Checksum
            if &actual != expected {
                return Err(PrebuiltAdapterCompileError::Checksum {
                    expected: expected.to_owned(),
                    actual: actual.to_owned(),
                }
                .into());
            }
        }

        // Set WASM file
        write(
            wasm_output_path, // path
            &wasm,            // contents
        )
        .map_err(|err| PrebuiltAdapterCompileError::WriteFile { source: err })?;

        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum PrebuiltAdapterCompileError {
    #[snafu(display("failed to read file"))]
    ReadFile { source: icp::fs::Error },

    #[snafu(transparent)]
    Url { source: url::ParseError },

    #[snafu(transparent)]
    Request { source: reqwest::Error },

    #[snafu(display("fetching {url} resulted in status-code: {code}"))]
    Status { url: Url, code: StatusCode },

    #[snafu(display(
        r#"
        resource has unexpected checksum.
            expected: {expected}
            actual: {actual}
        "#
    ))]
    Checksum { expected: String, actual: String },

    #[snafu(display("failed to write file"))]
    WriteFile { source: icp::fs::Error },
}
