//! Abstracted access to the ICP API.
//!
//! [`IcpAccess`] is a dumb transport: this crate encodes/decodes Candid and
//! decides *what* to call (including all management-canister calls), while the
//! implementation only routes bytes. Proxy routing is owned by the impl
//! (constructed with the proxy principal); the caller never threads a proxy
//! through per call.

use async_trait::async_trait;
use candid::Principal;
use snafu::Snafu;

#[derive(Debug, Snafu)]
pub enum IcpAccessError {
    #[snafu(display("update call to '{method}' on canister '{canister}' failed: {message}"))]
    Update {
        canister: Principal,
        method: String,
        message: String,
    },

    #[snafu(display("failed to read metadata '{path}' from canister '{canister}': {message}"))]
    ReadMetadata {
        canister: Principal,
        path: String,
        message: String,
    },
}

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait IcpAccess: Send + Sync {
    /// Perform an update call and return the raw reply bytes.
    ///
    /// `effective_canister_id` is the canister used for request routing. For a
    /// normal application call it equals `canister`; for a management-canister
    /// call (`canister == aaaaa-aa`) it must be the *target* canister, since the
    /// management canister has no routing of its own. `cycles` is attached to
    /// the call (only meaningful for proxied/funded calls such as
    /// `create_canister`).
    async fn canister_update(
        &self,
        canister: Principal,
        method: &str,
        arg: Vec<u8>,
        effective_canister_id: Principal,
        cycles: u128,
    ) -> Result<Vec<u8>, IcpAccessError>;

    /// Read a canister's custom-section metadata (via `read_state`). Returns
    /// `None` when the section is absent. Used for EOP-upgrade detection.
    async fn read_canister_metadata(
        &self,
        canister: Principal,
        path: &str,
    ) -> Result<Option<Vec<u8>>, IcpAccessError>;

    /// The caller's (identity's) principal.
    fn caller_principal(&self) -> Principal;
}
