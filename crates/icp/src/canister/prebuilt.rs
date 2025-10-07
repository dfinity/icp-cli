use std::str::FromStr;

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use reqwest::{Client, Method, Request};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::Sender;
use url::Url;

use crate::{
    canister::build::{Build, BuildError, Params, Step},
    fs::{read, write},
    manifest::adapter::prebuilt::SourceField,
};

pub struct Prebuilt;

#[async_trait]
impl Build for Prebuilt {
    async fn build(
        &self,
        step: &Step,
        params: &Params,
        _: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        // Adapter
        let adapter = match step {
            Step::Prebuilt(v) => v,
            _ => panic!("expected prebuilt adapter"),
        };

        let wasm = match &adapter.source {
            // Local path
            SourceField::Local(s) => {
                read(&params.path.join(&s.path)).context("failed to read prebuilt canister file")?
            }

            // Remote url
            SourceField::Remote(s) => {
                // Initialize a new http client
                let http_client = Client::new();

                // Parse Url
                let u = Url::from_str(&s.url).context("failed to parse prebuilt canister url")?;

                // Construct request
                let req = Request::new(
                    Method::GET,  // method
                    u.to_owned(), // url
                );

                // Execute request
                let resp = http_client
                    .execute(req)
                    .await
                    .context("failed to fetch prebuilt canister file")?;

                let status = resp.status();

                // Check for success
                if !status.is_success() {
                    return Err(anyhow!("http request failed {status}").into());
                }

                // Read response body
                resp.bytes()
                    .await
                    .context("failed to read http response")?
                    .to_vec()
            }
        };

        // Verify the checksum if it's provided
        if let Some(expected) = &adapter.sha256 {
            // Calculate checksum
            let actual = hex::encode({
                let mut h = Sha256::new();
                h.update(&wasm);
                h.finalize()
            });

            // Verify Checksum
            if &actual != expected {
                return Err(
                    anyhow!("checksum mismatch, expected: {expected}, actual: {actual}").into(),
                );
            }
        }

        // Set WASM file
        write(
            &params.output, // path
            &wasm,          // contents
        )
        .context("failed to write wasm output")?;

        Ok(())
    }
}
