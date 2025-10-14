use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    canister::{build, recipe::handlebars::HandlebarsError, sync},
    manifest::recipe::Recipe,
};

pub mod handlebars;

/// A recipe resolver takes a recipe that is specified in a canister manifest
/// and resolves it into a set of build/sync steps
#[async_trait]
pub trait Resolve: Sync + Send {
    #[allow(clippy::result_large_err)]
    async fn resolve(&self, recipe: &Recipe) -> Result<(build::Steps, sync::Steps), ResolveError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("field '{field}' contains an invalid value")]
    InvalidField { field: String },

    #[error("field '{field}' is required")]
    RequiredField { field: String },

    #[error("failed to resolve handlebars template")]
    Handlebars { source: HandlebarsError },
}

pub struct Resolver {
    pub handlebars: Arc<dyn Resolve>,
}

#[async_trait]
impl Resolve for Resolver {
    async fn resolve(&self, recipe: &Recipe) -> Result<(build::Steps, sync::Steps), ResolveError> {
        self.handlebars.resolve(recipe).await
    }
}
