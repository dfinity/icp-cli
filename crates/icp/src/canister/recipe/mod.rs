use async_trait::async_trait;
use snafu::prelude::*;

use crate::manifest::{
    canister::{BuildSteps, SyncSteps},
    recipe::Recipe,
};

pub mod handlebars;

/// Context passed to a recipe resolver, describing the canister being built.
///
/// Serializes to the shape injected into recipe templates under the `_` namespace:
///
/// ```yaml
/// canister:
///   name: <canister_name>
/// ```
pub struct RecipeContext {
    pub canister_name: String,
}

impl RecipeContext {
    /// Builds the YAML value injected into recipe templates under the `_` namespace.
    /// Constructing the mapping directly is infallible, unlike `serde` serialization.
    pub fn to_yaml(&self) -> serde_yaml::Value {
        use serde_yaml::{Mapping, Value};

        let mut canister = Mapping::new();
        canister.insert("name".into(), Value::String(self.canister_name.clone()));

        let mut root = Mapping::new();
        root.insert("canister".into(), Value::Mapping(canister));

        Value::Mapping(root)
    }
}

/// A recipe resolver takes a recipe that is specified in a canister manifest
/// and resolves it into a set of build/sync steps
#[async_trait]
pub trait Resolve: Sync + Send {
    #[allow(clippy::result_large_err)]
    async fn resolve(
        &self,
        recipe: &Recipe,
        recipe_context: &RecipeContext,
    ) -> Result<(BuildSteps, SyncSteps), ResolveError>;
}

#[derive(Debug, Snafu)]
pub enum ResolveError {
    #[snafu(display("failed to resolve handlebars template"))]
    Handlebars { source: handlebars::HandlebarsError },

    #[snafu(display("recipe resolution is not supported by this resolver: {recipe}"))]
    Unsupported { recipe: String },
}
