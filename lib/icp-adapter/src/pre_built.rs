use std::str::FromStr;

use crate::build::{Adapter, AdapterCompileError};
use async_trait::async_trait;
use camino::{Utf8Path, Utf8PathBuf};
use icp_fs::fs::{ReadFileError, WriteFileError, read, write};
use reqwest::{Client, Method, Request, StatusCode, Url};
use serde::Deserialize;
use snafu::Snafu;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SourceField {
    /// Local path on-disk to read a WASM file from
    Path(Utf8PathBuf),

    /// Remote Url to fetch a WASM file from
    Url(String),
}

/// Configuration for a Pre-built canister build adapter.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct PrebuiltAdapter {
    #[serde(flatten)]
    source: SourceField,
}

#[async_trait]
impl Adapter for PrebuiltAdapter {
    async fn compile(
        &self,
        _: &Utf8Path,
        wasm_output_path: &Utf8Path,
    ) -> Result<(), AdapterCompileError> {
        let wasm = match &self.source {
            // Local path
            SourceField::Path(p) => {
                read(p).map_err(|err| PrebuiltAdapterCompileError::ReadFile { source: err })?
            }

            // Remote url
            SourceField::Url(u) => {
                // Initialize a new http client
                let http_client = Client::new();

                // Parse Url
                let u = Url::from_str(u)
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
                let bs = resp
                    .bytes()
                    .await
                    .map_err(|err| PrebuiltAdapterCompileError::Request { source: err })?;

                bs.to_vec()
            }
        };

        // Set WASM file
        write(
            wasm_output_path, // path
            wasm,             // contents
        )
        .map_err(|err| PrebuiltAdapterCompileError::WriteFile { source: err })?;

        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum PrebuiltAdapterCompileError {
    #[snafu(transparent)]
    ReadFile { source: ReadFileError },

    #[snafu(transparent)]
    Url { source: url::ParseError },

    #[snafu(transparent)]
    Request { source: reqwest::Error },

    #[snafu(display("fetching {url} resulted in status-code: {code}"))]
    Status { url: Url, code: StatusCode },

    #[snafu(transparent)]
    WriteFile { source: WriteFileError },
}
