use std::{collections::HashMap, sync::Arc};

use crate::{store_artifact::ArtifactStore, store_id::IdStore, telemetry::EventLayer};
use camino::Utf8PathBuf;
use clap::Parser;
use commands::{Subcmd, DispatchError};
use console::Term;
use context::Context;
use icp_canister::{handlebars::Handlebars, recipe};
use icp_dirs::{DiscoverDirsError, IcpCliDirs};
use snafu::{Snafu, report};
use tracing::{Level, subscriber::set_global_default};
use tracing_subscriber::{
    Layer, Registry,
    filter::{self, FilterExt},
    fmt,
    layer::SubscriberExt,
};

mod commands;
mod context;
mod options;
mod progress;
mod store_artifact;
mod store_id;
mod telemetry;

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = ".icp/ids.json")]
    id_store: Utf8PathBuf,

    #[arg(long, default_value = ".icp/artifacts")]
    artifact_store: Utf8PathBuf,

    #[clap(long)]
    debug: bool,

    /// Generate markdown documentation for all commands and exit
    #[arg(long, hide = true)]
    markdown_help: bool,

    #[command(subcommand)]
    command: Option<Subcmd>,
}

#[tokio::main]
#[report]
async fn main() -> Result<(), ProgramError> {
    let cli = Cli::parse();

    // Generate markdown documentation if requested
    if cli.markdown_help {
        clap_markdown::print_help_markdown::<Cli>();
        return Ok(());
    }

    // Ensure a command was provided
    let command = cli.command.ok_or_else(|| ProgramError::Unexpected {
        err: "No subcommand provided. Use --help to see available commands.".to_string(),
    })?;

    // Printing for user-facing messages
    let term = Term::stdout();

    // Logging and Telemetry
    let (debug_layer, event_layer) = (
        fmt::layer(), // debug
        EventLayer,   // event
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
    set_global_default(reg).map_err(|err| ProgramError::Unexpected {
        err: err.to_string(),
    })?;

    // Setup project directory structure
    let dirs = IcpCliDirs::new()?;

    // Canister ID Store
    let ids = IdStore::new(&cli.id_store);

    // Canister Artifact Store (wasm)
    let artifacts = ArtifactStore::new(&cli.artifact_store);

    // Handlebar Templates (for recipes)
    let tmpls = recipe::TEMPLATES.map(|(name, tmpl)| (name.to_string(), tmpl.to_string()));

    // Prepare http client
    let http_client = reqwest::Client::new();

    // Recipes
    let recipe_resolver = Arc::new(recipe::Resolver {
        handlebars_resolver: Arc::new(Handlebars {
            recipes: HashMap::from_iter(tmpls),
            http_client,
        }),
    });

    // Setup environment
    let ctx = Context::new(
        term,      // term
        dirs,      // dirs
        ids,       // id_store
        artifacts, // artifact_store
        recipe_resolver,
    );

    commands::dispatch(&ctx, command).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum ProgramError {
    #[snafu(transparent)]
    Dispatch { source: DispatchError },

    #[snafu(transparent)]
    Dirs { source: DiscoverDirsError },

    #[snafu(display("an unexpected error occurred: {err}"))]
    Unexpected { err: String },
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
