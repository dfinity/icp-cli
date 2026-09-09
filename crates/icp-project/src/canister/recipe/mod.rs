//! Recipe resolution, split into two stages.
//!
//! [`Resolve`] retrieves a recipe's Handlebars template — reading a local file,
//! or downloading a remote URL or registry recipe — and returns the raw
//! template text. [`render`] turns that text into concrete build/sync steps.
//! The first stage does I/O and nothing else; the second is a pure function.
//!
//! The seam therefore covers only the fetching half, so a caller that already
//! has a template (or must not touch the network) can render without going
//! through a resolver at all.
//!
//! Caching a download is a third step, because whether a template is worth
//! keeping is not known until it renders: one bad remote response must not
//! become sticky in the cache. A resolver that wants to cache says so by
//! returning [`Fetched::deferred`], and the caller calls
//! [`Resolve::commit`] once rendering has succeeded. What is then written, and
//! where, is the resolver's own business — nothing about a cache crosses this
//! trait.

use async_trait::async_trait;
use snafu::Snafu;

use crate::manifest::recipe::Recipe;

pub mod render;

pub use render::{RecipeContext, RenderRecipeError, render_recipe};

/// A recipe template, as retrieved by a [`Resolve`].
pub struct Fetched {
    /// Raw Handlebars template source.
    pub template: String,

    /// Whether the resolver is holding work back until the template is known
    /// to render. A resolver that caches nothing leaves this `false` and is
    /// never asked to commit.
    pub deferred: bool,
}

/// A recipe template could not be retrieved or could not be cached.
///
/// Fetching one may mean an HTTP request and a write to a cache outside the
/// project. This layer knows only that it can fail, so the cause is carried
/// whole and displayed as itself.
#[derive(Debug, Snafu)]
#[snafu(transparent)]
pub struct ResolveError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

impl ResolveError {
    /// Wraps an implementation's own error for the trait boundary.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}

/// Retrieves the recipe templates a project references.
#[async_trait]
pub trait Resolve: Sync + Send {
    /// Fetch the Handlebars template for `recipe`.
    async fn resolve(&self, recipe: &Recipe) -> Result<Fetched, ResolveError>;

    /// Let the resolver finish whatever it deferred, now that the template it
    /// returned has rendered.
    ///
    /// Defaults to doing nothing, for a resolver that never defers.
    async fn commit(&self, recipe: &Recipe, fetched: &Fetched) -> Result<(), ResolveError> {
        let _ = (recipe, fetched);
        Ok(())
    }
}
