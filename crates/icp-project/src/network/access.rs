//! What a network is reached at, and how much its answers can be trusted.
//!
//! These are the values [`Access`](super::Access) hands back. *Producing* them
//! — reading a descriptor some launcher wrote, fetching a root key over HTTP —
//! is the implementation's business, not this crate's.

use serde::Serialize;
use url::Url;

/// Where a network's root key came from. Used for display so users can tell a
/// trusted/pinned key apart from one that was fetched trust-on-first-use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RootKeySource {
    /// Key belongs to a managed network we launched.
    Managed,
    /// The canonical IC mainnet root key.
    Mainnet,
    /// An explicit key pinned in the manifest or on the command line.
    Configured,
    /// Fetched from the network (trust-on-first-use, provenance unverified).
    Fetched,
}

/// The URLs a network is reached at, without any of the trust material
/// [`NetworkAccess`] carries. Resolving these never talks to the network, so a
/// caller that only needs to name an endpoint — to show it, or to hand it to a
/// sync plugin — does not trigger a root key fetch.
#[derive(Clone, Debug)]
pub struct NetworkUrls {
    /// Endpoint canister calls are submitted to.
    pub api_url: Url,

    /// Gateway that serves canisters over HTTP, if the network exposes one.
    pub http_gateway_url: Option<Url>,
}

#[derive(Clone)]
pub struct NetworkAccess {
    /// Network's (resolved) root key.
    pub root_key: Vec<u8>,

    /// Where [`Self::root_key`] came from.
    pub root_key_source: RootKeySource,

    /// Routing configuration
    pub api_url: Url,
    pub http_gateway_url: Option<Url>,

    /// If true, use friendly canister names with the gateway url
    pub use_friendly_domains: bool,
}
