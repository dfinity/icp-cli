use std::{env::current_dir, sync::Arc};

use anyhow::Error;
use clap::{CommandFactory, Parser};
use commands::{Context, Subcmd};
use icp::{
    Directories, agent,
    canister::{
        self, assets::Assets, build::Builder, prebuilt::Prebuilt, recipe::handlebars::Handlebars,
        script::Script, sync::Syncer,
    },
    identity, manifest, network,
    prelude::*,
    project,
};
use tracing::{Instrument, Level, debug, subscriber::set_global_default};
use tracing_subscriber::{
    Layer, Registry,
    filter::{self, FilterExt},
    layer::SubscriberExt,
};

use crate::{
    logging::logging_layer,
    store_artifact::ArtifactStore,
    store_id::IdStore,
    telemetry::EventLayer,
    version::{git_sha, icp_cli_version_str},
};

mod commands;
mod logging;
mod options;
mod progress;
mod store_artifact;
mod store_id;
mod telemetry;
mod version;

#[derive(Parser)]
#[command(version = icp_cli_version_str())]
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

    // Logging and Telemetry
    let reg = Registry::default().with(logging_layer(cli.debug)).with(
        EventLayer.with_filter(
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

    // Setup project directory structure
    let dirs = Directories::new()?;

    // Canister ID Store
    let ids = IdStore::new(&cli.id_store);

    // Canister Artifact Store (wasm)
    let artifacts = ArtifactStore::new(&cli.artifact_store);

    // Prepare http client
    let http_client = reqwest::Client::new();

    // Recipes
    let recipe = Arc::new(Handlebars { http_client });

    // Project Manifest Locator
    let mloc = Arc::new(manifest::Locator::new(
        current_dir()?.try_into()?, // cwd
        cli.project_dir,            // dir
    ));

    // Canister loader
    let cload = Arc::new(canister::PathLoader);

    // Builders/Syncers
    let cprebuilt = Arc::new(Prebuilt);
    let cassets = Arc::new(Assets);
    let cscript = Arc::new(Script);

    // Canister builder
    let builder = Arc::new(Builder {
        prebuilt: cprebuilt.to_owned(),
        script: cscript.to_owned(),
    });

    // Canister syncer
    let syncer = Arc::new(Syncer {
        assets: cassets.to_owned(),
        script: cscript.to_owned(),
    });

    // Project Loaders
    let ploaders = icp::ProjectLoaders {
        path: Arc::new(project::PathLoader),
        manifest: Arc::new(project::ManifestLoader {
            locate: mloc.clone(),
            recipe,
            canister: cload,
        }),
    };

    let pload = icp::Loader {
        locate: mloc.clone(),
        project: ploaders,
    };

    let pload = icp::Lazy::new(pload);
    let pload = Arc::new(pload);

    // Identity loader
    let idload = Arc::new(identity::Loader {
        dir: dirs.identity(),
    });

    // Network accessor
    let netaccess = Arc::new(network::Accessor {
        project: mloc.clone(),
        descriptors: dirs.port_descriptor(),
    });

    // Agent creator
    let agent_creator = Arc::new(agent::Creator);

    // Setup environment
    let ctx = Context {
        workspace: mloc,
        dirs,
        ids,
        artifacts,
        project: pload,
        identity: idload,
        network: netaccess,
        agent: agent_creator,
        builder,
        syncer,
        debug: cli.debug,
    };

    // Execute the command within a span that includes version and SHA context
    let trace_span = tracing::trace_span!(
        "icp-cli",
        version = icp_cli_version_str(),
        git_sha = git_sha()
    );

    debug!(
        version = icp_cli_version_str(),
        git_sha = git_sha(),
        command = ?command,
        "Starting icp-cli"
    );

    commands::dispatch(&ctx, command)
        .instrument(trace_span)
        .await?;

    debug!("Command executed successfully");

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
