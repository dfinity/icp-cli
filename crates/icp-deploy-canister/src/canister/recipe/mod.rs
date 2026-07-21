use async_trait::async_trait;
use snafu::prelude::*;

use crate::manifest::{
    canister::{BuildSteps, SyncSteps},
    recipe::Recipe,
};

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
/// and resolves it into a set of build/sync steps.
///
/// The concrete resolver (which fetches templates over HTTP and renders them)
/// lives in the host `icp` crate; this crate only defines the interface so that
/// consolidation can call an injected resolver.
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
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
    /// The injected resolver failed. The concrete source (e.g. a Handlebars or
    /// HTTP error from the host resolver) is boxed because this crate does not
    /// depend on the resolver's implementation.
    #[snafu(display("failed to resolve recipe"))]
    Resolve {
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}
