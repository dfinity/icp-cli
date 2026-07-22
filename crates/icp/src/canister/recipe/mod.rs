//! Host-side recipe facade.
//!
//! The `RemoteResourceResolve` interface + context/error types live in
//! `icp_deploy_canister::canister::recipe`; the concrete Handlebars resolver
//! (which fetches templates and plugin wasms over HTTP and caches them) stays
//! here.

pub use icp_deploy_canister::canister::recipe::{
    RecipeContext, RemoteResourceResolve, ResolveError,
};

pub mod handlebars;
