use camino::Utf8PathBuf;
use candid::Principal;
use ic_agent::Agent;
use icp_sync_plugin::{RunPluginError, run_plugin};
use reqwest::{Client, Method, Request};
use sha2::{Digest, Sha256};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;
use url::Url;

use crate::{
    fs::{read, read_to_string, write},
    manifest::adapter::{plugin::Adapter, prebuilt::SourceField},
};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PluginError {
    #[snafu(display("failed to read plugin wasm at '{path}'"))]
    ReadWasm {
        source: crate::fs::IoError,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to read plugin input file at '{path}'"))]
    ReadFile {
        source: crate::fs::IoError,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to parse plugin url"))]
    ParseUrl { source: url::ParseError },

    #[snafu(display("failed to fetch plugin wasm file"))]
    HttpRequest { source: reqwest::Error },

    #[snafu(display("http request failed: {status}"))]
    HttpStatus { status: reqwest::StatusCode },

    #[snafu(display("failed to read http response for plugin"))]
    HttpResponse { source: reqwest::Error },

    #[snafu(display("failed to write downloaded plugin wasm to temp file"))]
    WriteTempWasm { source: crate::fs::IoError },

    #[snafu(display("plugin wasm checksum mismatch, expected: {expected}, actual: {actual}"))]
    ChecksumMismatch { expected: String, actual: String },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetIdentityPrincipal { err: String },

    #[snafu(display("failed to run plugin"))]
    Run { source: RunPluginError },

    #[snafu(display("failed to send log message"))]
    Log {
        source: tokio::sync::mpsc::error::SendError<String>,
    },
}

pub(super) async fn sync(
    adapter: &Adapter,
    params: &Params,
    agent: &Agent,
    environment: &str,
    proxy: Option<Principal>,
    stdio: Option<Sender<String>>,
) -> Result<(), PluginError> {
    // 1. Acquire the wasm bytes — either from a local path or a remote URL.
    let (wasm_bytes, wasm_path) = match &adapter.source {
        SourceField::Local(s) => {
            let full_path = params.path.join(&s.path);
            if let Some(tx) = &stdio {
                tx.send(format!("Reading plugin wasm: {full_path}"))
                    .await
                    .context(LogSnafu)?;
            }
            let bytes = read(full_path.as_ref()).context(ReadWasmSnafu {
                path: full_path.clone(),
            })?;
            (bytes, full_path)
        }

        SourceField::Remote(s) => {
            let url = Url::parse(&s.url).context(ParseUrlSnafu)?;
            if let Some(tx) = &stdio {
                tx.send(format!("Fetching plugin wasm: {url}"))
                    .await
                    .context(LogSnafu)?;
            }
            let client = Client::new();
            let req = Request::new(Method::GET, url);
            let resp = client.execute(req).await.context(HttpRequestSnafu)?;
            let status = resp.status();
            if !status.is_success() {
                return HttpStatusSnafu { status }.fail();
            }
            let bytes = resp.bytes().await.context(HttpResponseSnafu)?.to_vec();

            // Write to a temp file so we can pass a path to `run_plugin`.
            let tmp_path = params.path.join(format!(
                ".icp-plugin-{}.wasm",
                hex::encode(&bytes[..std::cmp::min(8, bytes.len())])
            ));
            write(tmp_path.as_ref(), &bytes).context(WriteTempWasmSnafu)?;
            (bytes, tmp_path)
        }
    };

    // 2. Verify sha256 checksum if provided.
    let cksum = hex::encode({
        let mut h = Sha256::new();
        h.update(&wasm_bytes);
        h.finalize()
    });

    if let Some(expected) = &adapter.sha256 {
        if let Some(tx) = &stdio {
            tx.send("Verifying plugin wasm checksum".to_string())
                .await
                .context(LogSnafu)?;
        }
        if &cksum != expected {
            return ChecksumMismatchSnafu {
                expected: expected.clone(),
                actual: cksum,
            }
            .fail();
        }
    }

    // 3. Collect inputs: `dirs` stays as manifest strings (runtime preopens them),
    //    `files` are read on the host and passed inline.
    let base_dir = Utf8PathBuf::from(params.path.as_str());
    let dirs: Vec<String> = adapter.dirs.clone().unwrap_or_default();

    let mut files: Vec<(String, String)> = Vec::new();
    for name in adapter.files.as_deref().unwrap_or(&[]) {
        let abs = params.path.join(name);
        let content = read_to_string(abs.as_ref()).context(ReadFileSnafu { path: abs })?;
        files.push((name.clone(), content));
    }

    // 4. Run the plugin (blocking call — signal Tokio that this thread will block).
    let identity_principal = agent
        .get_principal()
        .map_err(|err| PluginError::GetIdentityPrincipal { err })?;

    let wasm_path_buf = Utf8PathBuf::from(wasm_path.as_str());
    let agent_clone = agent.clone();
    let environment_owned = environment.to_owned();
    let stdio_clone = stdio.clone();

    tokio::task::block_in_place(|| {
        run_plugin(
            wasm_path_buf,
            base_dir,
            dirs,
            files,
            params.cid,
            agent_clone,
            proxy,
            identity_principal,
            environment_owned,
            stdio_clone,
        )
    })
    .context(RunSnafu)?;

    // Clean up temp file if we downloaded from a remote URL.
    if matches!(&adapter.source, SourceField::Remote(_)) {
        let _ = std::fs::remove_file(wasm_path.as_std_path());
    }

    Ok(())
}
