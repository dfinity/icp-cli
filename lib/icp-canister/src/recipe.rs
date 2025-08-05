use icp_adapter::{
    assets::{AssetsAdapter, DirField},
    pre_built::{PrebuiltAdapter, RemoteSource, SourceField},
    script::{CommandField, ScriptAdapter},
};
use mockall::automock;
use serde_yaml::Value;
use snafu::Snafu;

use crate::{BuildStep, BuildSteps, Recipe, SyncStep, SyncSteps, manifest::RecipeType};

#[derive(Debug, Snafu)]
pub enum ResolveError {
    #[snafu(display("field '{field}' contains an invalid value"))]
    InvalidField { field: String },

    #[snafu(display("failed to resolve recipe into build/sync steps"))]
    Resolve,
}

/// A recipe resolver takes a recipe that is specified in a canister manifest
/// and resolves it into a set of build/sync steps
#[automock]
pub trait Resolve: Sync + Send {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError>;
}

pub struct Resolver;

impl Resolve for Resolver {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        match recipe.recipe_type {
            RecipeType::Assets => (Assets).resolve(recipe),
            RecipeType::Motoko => (Motoko).resolve(recipe),
            RecipeType::Rust => (Rust).resolve(recipe),
        }
    }
}

pub struct Assets;

impl Resolve for Assets {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        // Version
        let version = match recipe.instructions.get("version") {
            // parse provided valuer
            Some(v) => Value::as_str(v).ok_or(ResolveError::InvalidField {
                field: "version".to_string(),
            })?,

            // fallback to default
            None => "0.27.0",
        };

        // Directory
        let dir = match recipe.instructions.get("dir") {
            // parse provided valuer
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

        let build = BuildSteps {
            steps: vec![BuildStep::Prebuilt(PrebuiltAdapter {
                source: SourceField::Remote(RemoteSource { url }),
                sha256: None,
            })],
        };

        // Sync
        let sync = SyncSteps {
            steps: vec![SyncStep::Assets(AssetsAdapter {
                dir: DirField::Dir(dir.to_string()),
            })],
        };

        Ok((build, sync))
    }
}

pub struct Motoko;

impl Resolve for Motoko {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        // main entry point for the motoko program
        let entry = match recipe.instructions.get("entry") {
            // parse provided valuer
            Some(v) => Value::as_str(v).ok_or(ResolveError::InvalidField {
                field: "entry".to_string(),
            })?,

            // fallback to default
            None => "main.mo",
        };

        // Build
        let build = BuildSteps {
            steps: vec![BuildStep::Script(ScriptAdapter {
                command: CommandField::Commands(vec![
                    r#"sh -c 'command -v moc >/dev/null 2>&1 || { echo >&2 "moc not found. To install moc, see https://internetcomputer.org/docs/building-apps/getting-started/install \n"; exit 1; }'"#.to_string(),
                    format!(r#"sh -c 'moc {entry}'"#),
                    r#"sh -c 'mv main.wasm "$ICP_WASM_OUTPUT_PATH"'"#.to_string(),
                ]),
            })],
        };

        // Sync
        let sync = SyncSteps { steps: vec![] };

        Ok((build, sync))
    }
}

pub struct Rust;

impl Resolve for Rust {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        // Build
        let build = BuildSteps { steps: vec![] };

        // Sync
        let sync = SyncSteps { steps: vec![] };

        Ok((build, sync))
    }
}

pub struct Handlebars;

impl Resolve for Handlebars {
    fn resolve(&self, recipe: &Recipe) -> Result<(BuildSteps, SyncSteps), ResolveError> {
        // Build
        let build = BuildSteps { steps: vec![] };

        // Sync
        let sync = SyncSteps { steps: vec![] };

        Ok((build, sync))
    }
}
