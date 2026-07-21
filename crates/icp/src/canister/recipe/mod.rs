//! Host-side recipe facade.
//!
//! The recipe `Resolve` interface + context/error types live in
//! `icp_deploy_canister::canister::recipe`; the concrete Handlebars resolver
//! (which fetches templates over HTTP) stays here.

pub use icp_deploy_canister::canister::recipe::{RecipeContext, Resolve, ResolveError};

pub mod handlebars;
