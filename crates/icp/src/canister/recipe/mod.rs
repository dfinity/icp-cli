//! Recipe resolution, split into two stages.
//!
//! [`fetch`] retrieves a recipe's Handlebars template — reading a local file, or
//! downloading and caching a remote URL or registry recipe — and returns the raw
//! template text. [`render`] turns that text into concrete build/sync steps. The
//! first stage does I/O and nothing else; the second is a pure function.
//!
//! The [`Resolve`] seam therefore covers only the fetching half, so a caller that
//! already has a template (or must not touch the network) can render without
//! going through a resolver at all.

use async_trait::async_trait;
use snafu::prelude::*;

use crate::manifest::recipe::Recipe;

pub mod fetch;
pub mod render;

pub use render::{RecipeContext, RenderRecipeError, render_recipe};

/// Retrieves the recipe templates a project references.
///
/// Only *fetching* is behind this trait: rendering a fetched template into build
/// and sync steps is [`render_recipe`], which needs no I/O and so needs no seam.
#[async_trait]
pub trait Resolve: Sync + Send {
    /// Fetch the Handlebars template for `recipe`, returning its raw source.
    async fn resolve(&self, recipe: &Recipe) -> Result<String, ResolveError>;
}

#[derive(Debug, Snafu)]
pub enum ResolveError {
    #[snafu(display("failed to fetch recipe template"))]
    Fetch { source: fetch::RecipeFetchError },
}
