//! Recipe resolution, split into two stages.
//!
//! [`fetch`] retrieves a recipe's Handlebars template — reading a local file, or
//! downloading a remote URL or registry recipe — and returns the raw template
//! text. [`render`] turns that text into concrete build/sync steps. The first
//! stage does I/O and nothing else; the second is a pure function.
//!
//! The [`Resolve`] seam therefore covers only the fetching half, so a caller that
//! already has a template (or must not touch the network) can render without
//! going through a resolver at all.
//!
//! Caching a download is a third step, because whether a template is worth
//! keeping is not known until it renders. A download that carried a `sha256` is
//! cached during the fetch — the checksum already proves the bytes are the ones
//! that were asked for. An *unpinned* download is held back as a
//! [`PendingCache`] and only committed by the caller once rendering succeeds, so
//! that one bad remote response cannot become sticky in the cache. The full
//! sequence is therefore fetch → render → [`Resolve::commit`].

use async_trait::async_trait;
use snafu::prelude::*;

use crate::manifest::recipe::Recipe;

pub mod fetch;
pub mod render;

pub use fetch::{Fetched, PendingCache};
pub use render::{RecipeContext, RenderRecipeError, render_recipe};

/// Retrieves the recipe templates a project references.
///
/// Only *fetching* is behind this trait: rendering a fetched template into build
/// and sync steps is [`render_recipe`], which needs no I/O and so needs no seam.
#[async_trait]
pub trait Resolve: Sync + Send {
    /// Fetch the Handlebars template for `recipe`, returning its raw source and
    /// any cache write held back until the template is known to render.
    async fn resolve(&self, recipe: &Recipe) -> Result<Fetched, ResolveError>;

    /// Write a held-back download to the cache, now that it has rendered.
    ///
    /// Defaults to doing nothing: only [`fetch::RecipeFetcher`] caches, and only
    /// it can construct the [`PendingCache`] that reaches this method, so a
    /// resolver that never defers a write never has one to commit.
    async fn commit(&self, pending: PendingCache) -> Result<(), ResolveError> {
        let _ = pending;
        Ok(())
    }
}

#[derive(Debug, Snafu)]
pub enum ResolveError {
    #[snafu(display("failed to fetch recipe template"))]
    Fetch { source: fetch::RecipeFetchError },

    #[snafu(display("failed to cache recipe template"))]
    Commit { source: fetch::RecipeFetchError },
}
