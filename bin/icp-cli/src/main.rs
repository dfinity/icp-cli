use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use crate::{store_artifact::ArtifactStore, store_id::IdStore, telemetry::EventLayer};
use camino::Utf8PathBuf;
use clap::Parser;
use commands::{Cmd, DispatchError};
use console::Term;
use context::Context;
use icp_canister::{handlebars::Handlebars, recipe};
use icp_dirs::{DiscoverDirsError, IcpCliDirs};
use indicatif::ProgressStyle;
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

    #[command(flatten)]
    command: Cmd,
}

#[tokio::main]
#[report]
async fn main() -> Result<(), ProgramError> {
    let cli = Cli::parse();

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

    commands::dispatch(&ctx, cli.command).await?;

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

// Animation frames for the spinner - creates a rotating star effect
const TICKS: &[&str] = &["✶", "✸", "✹", "✺", "✹", "✷"];

// Final tick symbols for different completion states
const TICK_EMPTY: &str = " ";
const TICK_SUCCESS: &str = "✔";
const TICK_FAILURE: &str = "✘";

// Color schemes for different progress states
const COLOR_REGULAR: &str = "blue";
const COLOR_SUCCESS: &str = "green";
const COLOR_FAILURE: &str = "red";

// Creates a progress bar style with a spinner that transitions to a final tick symbol
// - end_tick: the symbol to display when the progress completes (success, failure, etc.)
// - color: the color theme for the spinner and text
fn make_style(end_tick: &str, color: &str) -> ProgressStyle {
    // Template format: "[prefix] [spinner] [message]"
    let tmpl = format!("{{prefix}} {{spinner:.{color}}} {{msg}}");

    ProgressStyle::with_template(&tmpl)
        .expect("invalid style template")
        // Combine animation frames with the final completion symbol
        .tick_strings(&[TICKS, &[end_tick]].concat())
}

/// A fixed-capacity rolling buffer that always holds the last `capacity` items.
#[derive(Debug)]
pub struct RollingLines {
    buf: VecDeque<String>,
    capacity: usize,
}

impl RollingLines {
    /// Create a new buffer with a fixed capacity, pre-filled with empty strings.
    pub fn new(capacity: usize) -> Self {
        let mut buf = VecDeque::with_capacity(capacity);

        for _ in 0..capacity {
            buf.push_back(String::new());
        }

        Self { buf, capacity }
    }

    /// Push a new line, evicting the oldest if full.
    pub fn push(&mut self, line: String) {
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }

        self.buf.push_back(line);
    }

    /// Get an iterator over the current contents (in order).
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.buf.iter().map(|s| s.as_str())
    }
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
