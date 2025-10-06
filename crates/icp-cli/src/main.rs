use std::{collections::HashMap, env::current_dir, sync::Arc};

use anyhow::Error;
use clap::{CommandFactory, Parser};
use commands::Subcmd;
use console::Term;
use context::Context;
use icp::{
    Directories,
    canister::{
        self,
        recipe::{
            self,
            handlebars::{Handlebars, TEMPLATES},
        },
    },
    environment, manifest, network,
    prelude::*,
};
use tracing::{Level, subscriber::set_global_default};
use tracing_subscriber::{
    Layer, Registry,
    filter::{self, FilterExt},
    layer::SubscriberExt,
};

use crate::{store_artifact::ArtifactStore, store_id::IdStore, telemetry::EventLayer};

mod commands;
mod context;
mod options;
mod progress;
mod store_artifact;
mod store_id;
mod telemetry;

#[derive(Parser)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Directory to use as your project base directory. If not specified the directory structure is traversed up until an icp.yaml file is found"
    )]
    project_dir: Option<PathBuf>,

    #[arg(long, default_value = ".icp/ids.json")]
    id_store: PathBuf,

    #[arg(long, default_value = ".icp/artifacts")]
    artifact_store: PathBuf,

    /// Enable debug logging
    #[arg(long, default_value = "false", global = true)]
    debug: bool,

    /// Generate markdown documentation for all commands and exit
    #[arg(long, hide = true)]
    markdown_help: bool,

    #[command(subcommand)]
    command: Option<Subcmd>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    // Generate markdown documentation if requested
    if cli.markdown_help {
        clap_markdown::print_help_markdown::<Cli>();
        return Ok(());
    }

    // If no command was provided, print help and exit
    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            Cli::command().print_help()?;
            return Ok(());
        }
    };

    // Printing for user-facing messages
    let term = Term::stdout();

    // Logging and Telemetry
    let (debug_layer, event_layer) = (
        tracing_subscriber::fmt::layer(), // debug
        EventLayer,                       // event
    );

    let reg = Registry::default()
        .with(
            debug_layer.with_filter(
                filter::filter_fn(|_| true)
                    //
                    // Only log if `debug` is set
                    .and(filter::filter_fn(move |_| cli.debug))
                    //
                    // Only log if event level is debug
                    .and(filter::filter_fn(|md| md.level() == &Level::DEBUG)),
            ),
        )
        .with(
            event_layer.with_filter(
                filter::filter_fn(|_| true)
                    //
                    // Only log to telemetry layer if target is `events`
                    .and(filter::filter_fn(move |md| md.target() == "events"))
                    //
                    // Only log to telemetry layer if level if `trace`
                    .and(filter::filter_fn(|md| md.level() == &Level::TRACE)),
            ),
        );

    // Set the configured subscriber registry as the global default for tracing
    // This enables the logging and telemetry layers we configured above
    set_global_default(reg)?;

    // Project Manifest (Locator and Loader)
    let mloc = manifest::Locator::new(
        current_dir()?.try_into()?, // cwd
        cli.project_dir,            // dir
    );

    let mload = manifest::Loader::new(Arc::new(mloc));
    let mload = Arc::new(mload);

    // Setup project directory structure
    let dirs = Directories::new()?;

    // Canister ID Store
    let ids = IdStore::new(&cli.id_store);

    // Canister Artifact Store (wasm)
    let artifacts = ArtifactStore::new(&cli.artifact_store);

    // Handlebar Templates (for recipes)
    let tmpls = TEMPLATES.map(|(name, tmpl)| (name.to_string(), tmpl.to_string()));

    // Prepare http client
    let http_client = reqwest::Client::new();

    // Recipes
    let recipe = Arc::new(recipe::Resolver {
        handlebars: Arc::new(Handlebars {
            recipes: HashMap::from_iter(tmpls),
            http_client,
        }),
    });

    // Canister loader
    let cload = canister::ManifestLoader::new(recipe);
    let cload = Arc::new(cload);

    // Network loader
    let nload = Arc::new(network::Loader);

    // Environment loader
    let eload = Arc::new(environment::Loader);

    // Canister builder
    let cbuild = Arc::new(canister::Builder);

    // Canister syncer
    let csync = Arc::new(canister::Syncer);

    // Project Loader
    let pload = icp::Loader::new(mload, cload, nload, eload);
    let pload = icp::Lazy::new(pload);
    let pload = Arc::new(pload);

    // Setup environment
    let ctx = Context::new(
        term,      // term
        dirs,      // dirs
        ids,       // id_store
        artifacts, // artifact_store
        pload,     // project
    );

    commands::dispatch(&ctx, command).await?;

    Ok(())
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
