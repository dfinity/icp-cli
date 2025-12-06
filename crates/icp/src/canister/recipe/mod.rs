use async_trait::async_trait;
use snafu::prelude::*;

use crate::manifest::{
    canister::{BuildSteps, SyncSteps},
    recipe::Recipe,
};

pub mod handlebars;

/// A recipe resolver takes a recipe that is specified in a canister manifest
/// and resolves it into a set of build/sync steps
#[async_trait]
pub trait Resolve: Sync + Send {
    #[allow(clippy::result_large_err)]
    async fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError>;
}

#[derive(Debug, Snafu)]
pub enum ResolveError {
    #[snafu(display("failed to resolve handlebars template"))]
    Handlebars { source: handlebars::HandlebarsError },
}
