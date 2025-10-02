use std::sync::Arc;

use async_trait::async_trait;
use serde_yaml::Value;

use crate::{
    canister::{build, recipe::handlebars::HandlebarsError, sync},
    manifest::{
        adapter::{
            assets::{self, DirField},
            prebuilt::{self, RemoteSource, SourceField},
            script::{self, CommandField},
        },
        recipe::{Recipe, RecipeType},
    },
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
        match recipe.recipe_type {
            RecipeType::Assets => (Assets).resolve(recipe),
            RecipeType::Motoko => (Motoko).resolve(recipe),
            RecipeType::Rust => (Rust).resolve(recipe),

            // For unknown recipe types, delegate to the Handlebars resolver
            // This allows for extensible recipe types defined via templates
            RecipeType::Unknown(_) => self.handlebars.resolve(recipe),
        }
        .await
    }
}

pub struct Assets;

#[async_trait]
impl Resolve for Assets {
    async fn resolve(&self, recipe: &Recipe) -> Result<(build::Steps, sync::Steps), ResolveError> {
        // Version
        let version = match recipe.configuration.get("version") {
            // parse provided value
            Some(v) => Value::as_str(v).ok_or(ResolveError::InvalidField {
                field: "version".to_string(),
            })?,

            // fallback to default
            None => "0.27.0",
        };

        // Directory
        let dir = match recipe.configuration.get("dir") {
            // parse provided value
            Some(v) => Value::as_str(v).ok_or(ResolveError::InvalidField {
                field: "dir".to_string(),
            })?,

            // fallback to default
            None => "www",
        };

        // Build
        let url = format!(
            "https://github.com/dfinity/sdk/raw/refs/tags/{version}/src/distributed/assetstorage.wasm.gz",
        );

        let build = build::Steps {
            steps: vec![build::Step::Prebuilt(prebuilt::Adapter {
                source: SourceField::Remote(RemoteSource { url }),
                sha256: None,
            })],
        };

        // Sync
        let sync = sync::Steps {
            steps: vec![sync::Step::Assets(assets::Adapter {
                dir: DirField::Dir(dir.to_string()),
            })],
        };

        Ok((build, sync))
    }
}

pub struct Motoko;

#[async_trait]
impl Resolve for Motoko {
    async fn resolve(&self, recipe: &Recipe) -> Result<(build::Steps, sync::Steps), ResolveError> {
        // main entry point for the motoko program
        let entry = match recipe.configuration.get("entry") {
            // parse provided value
            Some(v) => Value::as_str(v).ok_or(ResolveError::InvalidField {
                field: "entry".to_string(),
            })?,

            // fallback to default
            None => "main.mo",
        };

        // Build
        let build = build::Steps {
            steps: vec![build::Step::Script(script::Adapter {
                command: CommandField::Commands(vec![
                    r#"sh -c 'command -v moc >/dev/null 2>&1 || { echo >&2 "moc not found. To install moc, see https://internetcomputer.org/docs/building-apps/getting-started/install \n"; exit 1; }'"#.to_string(),
                    format!(r#"sh -c 'moc {entry}'"#),
                    r#"sh -c 'mv main.wasm "$ICP_WASM_OUTPUT_PATH"'"#.to_string(),
                ]),
            })],
        };

        // Sync
        let sync = sync::Steps { steps: vec![] };

        Ok((build, sync))
    }
}

pub struct Rust;

#[async_trait]
impl Resolve for Rust {
    async fn resolve(&self, recipe: &Recipe) -> Result<(build::Steps, sync::Steps), ResolveError> {
        // Canister's cargo package
        let package = match recipe.configuration.get("package") {
            // parse provided value
            Some(v) => Value::as_str(v).ok_or(ResolveError::InvalidField {
                field: "package".to_string(),
            })?,

            // raise error otherwise
            None => Err(ResolveError::RequiredField {
                field: "package".to_string(),
            })?,
        };

        // Build
        let package_arg = format!("--package {package}");
        let output_name = format!("{}.wasm", package.replace("-", "_"));

        let build = build::Steps {
            steps: vec![build::Step::Script(script::Adapter {
                command: CommandField::Commands(vec![
                    format!(
                        r#"cargo build {package_arg} --target wasm32-unknown-unknown --release"#
                    ),
                    format!(
                        r#"sh -c 'mv target/wasm32-unknown-unknown/release/{output_name} "$ICP_WASM_OUTPUT_PATH"'"#
                    ),
                ]),
            })],
        };

        // Sync
        let sync = sync::Steps { steps: vec![] };

        Ok((build, sync))
    }
}
