//! Abstracted HTTP access.
//!
//! Reserved for programmatic/canister callers. The current install/sync/deploy
//! core does not fetch over HTTP — remote wasm downloads and recipe resolution
//! stay in the host `icp` crate — but [`HttpAccess`] is threaded through the
//! `deploy` entry points to keep the public API stable.

use async_trait::async_trait;
use snafu::Snafu;
use url::Url;

#[derive(Debug, Snafu)]
pub enum HttpAccessError {
    #[snafu(display("HTTP GET of '{url}' failed: {message}"))]
    Get { url: Url, message: String },
}

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait HttpAccess: Send + Sync {
    async fn http_get(&self, url: &Url) -> Result<Vec<u8>, HttpAccessError>;
}
