//! Host-side recipe facade.
//!
//! The [`RemoteResourceResolve`] interface, recipe rendering, and the
//! context/error types live in [`icp_deploy_canister::canister::recipe`]; the
//! concrete resolver — which fetches templates and plugin wasms over HTTP and
//! caches them — stays here in [`resolver`].
//!
//! Resolution is therefore staged: fetch the template, render it, then commit
//! whatever the resolver held back. See [`resolver::ResourceResolver`] for why
//! the cache write waits.

pub use icp_deploy_canister::canister::recipe::{
    FetchedRecipe, NoResolve, RecipeContext, RemoteResourceResolve, RenderRecipeError,
    ResolveError, render_recipe,
};

pub mod resolver;
