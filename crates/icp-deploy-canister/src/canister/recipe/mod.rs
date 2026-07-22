use async_trait::async_trait;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::manifest::{
    adapter::prebuilt::SourceField,
    canister::{BuildSteps, SyncSteps},
    recipe::Recipe,
};
use crate::prelude::*;

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

/// Resolves the remote resources a project references — recipe templates and
/// plugin wasms — into local, usable form, fetching over HTTP and caching on
/// disk as needed.
///
/// The concrete resolver (which owns the HTTP client and the package cache)
/// lives in the host `icp` crate; this crate defines the interface so that
/// consolidation and sync can call an injected resolver.
#[async_trait]
pub trait RemoteResourceResolve: Sync + Send {
    /// Resolve a recipe into concrete build/sync steps (fetch + render).
    #[allow(clippy::result_large_err)]
    async fn resolve_recipe(
        &self,
        recipe: &Recipe,
        recipe_context: &RecipeContext,
    ) -> Result<(BuildSteps, SyncSteps), ResolveError>;

    /// Resolve a plugin wasm `source` (relative to `base_dir`) to a local path,
    /// verifying `sha256` and caching a remote download. `stdio` receives
    /// progress lines.
    async fn resolve_wasm(
        &self,
        source: &SourceField,
        base_dir: &Path,
        sha256: Option<&str>,
        stdio: Option<Sender<String>>,
    ) -> Result<PathBuf, ResolveError>;
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

    #[snafu(display("failed to resolve plugin wasm"))]
    ResolveWasm {
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}
