//! Host-side recipe facade.
//!
//! The `RemoteResourceResolve` interface, recipe rendering, and context/error
//! types live in `icp_deploy_canister::canister::recipe`; the concrete resolver
//! (which fetches templates and plugin wasms over HTTP and caches them) stays
//! here.

pub use icp_deploy_canister::canister::recipe::{
    RecipeContext, RemoteResourceResolve, ResolveError,
};

pub mod resolver;
