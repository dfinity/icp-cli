use std::{collections::HashMap, sync::Arc};

use crate::{store_artifact::ArtifactStore, store_id::IdStore};
use camino::Utf8PathBuf;
use clap::Parser;
use commands::{Cmd, DispatchError};
use context::Context;
use icp_canister::{handlebars::Handlebars, recipe};
use icp_dirs::{DiscoverDirsError, IcpCliDirs};
use snafu::{Snafu, report};

mod commands;
mod context;
mod options;
mod store_artifact;
mod store_id;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = ".icp/ids.json")]
    id_store: Utf8PathBuf,

    #[arg(long, default_value = ".icp/artifacts")]
    artifact_store: Utf8PathBuf,

    #[command(flatten)]
    command: Cmd,
}

#[tokio::main]
#[report]
async fn main() -> Result<(), ProgramError> {
    let cli = Cli::parse();

    // Setup project directory structure
    let dirs = IcpCliDirs::new()?;

    // Canister ID Store
    let ids = IdStore::new(&cli.id_store);

    // Canister Artifact Store (wasm)
    let artifacts = ArtifactStore::new(&cli.artifact_store);

    // Handlebar Templates (for recipes)
    let tmpls = recipe::TEMPLATES.map(|(name, tmpl)| (name.to_string(), tmpl.to_string()));

    // Recipes
    let recipe_resolver = Arc::new(recipe::Resolver {
        handlebars_resolver: Arc::new(Handlebars {
            recipes: HashMap::from_iter(tmpls),
        }),
    });

    // Setup environment
    let ctx = Context::new(
        dirs,      // dirs
        ids,       // id_store
        artifacts, // artifact_store
        recipe_resolver,
    );

    commands::dispatch(&ctx, cli.command).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum ProgramError {
    #[snafu(transparent)]
    Dispatch { source: DispatchError },

    #[snafu(transparent)]
    Dirs { source: DiscoverDirsError },
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    #[test]
    fn valid_command() {
        Cli::command().debug_assert();
    }
}
