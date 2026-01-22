use anyhow::Error;
use clap::{CommandFactory, Parser};
use commands::Command;
use console::Term;
use icp::{context::TermWriter, prelude::*};
use tracing::{Instrument, Level, debug, subscriber::set_global_default, trace_span};
use tracing_subscriber::{
    Layer, Registry,
    filter::{self, FilterExt},
    layer::SubscriberExt,
};

use crate::{
    logging::debug_layer,
    telemetry::EventLayer,
    version::{git_sha, icp_cli_version_str},
};

mod artifacts;
mod commands;
mod logging;
pub(crate) mod operations;
mod options;
mod progress;
mod telemetry;
mod version;

/// Styles from <https://github.com/rust-lang/cargo/blob/master/src/cargo/util/style.rs>
mod style {
    use anstyle::*;
    use clap::builder::Styles;

    const HEADER: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
    const USAGE: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
    const LITERAL: Style = AnsiColor::Cyan.on_default().effects(Effects::BOLD);
    const PLACEHOLDER: Style = AnsiColor::Cyan.on_default();
    const ERROR: Style = AnsiColor::Red.on_default().effects(Effects::BOLD);
    const VALID: Style = AnsiColor::Cyan.on_default().effects(Effects::BOLD);
    const INVALID: Style = AnsiColor::Yellow.on_default().effects(Effects::BOLD);

    pub const STYLES: Styles = {
        Styles::styled()
            .header(HEADER)
            .usage(USAGE)
            .literal(LITERAL)
            .placeholder(PLACEHOLDER)
            .error(ERROR)
            .valid(VALID)
            .invalid(INVALID)
            .error(ERROR)
    };
}

mod heading {
    pub const GLOBAL_PARAMETERS: &str = "Common Parameters";
}

#[derive(Parser)]
#[command(
    name = "icp",
    version,
    arg_required_else_help(true),
    about,
    next_line_help(false),
    styles(style::STYLES)
)]
struct Cli {
    /// Directory to use as your project root directory.
    /// If not specified the directory structure is traversed up until an icp.yaml file is found
    #[arg(
        long,
        global = true,
        help_heading = heading::GLOBAL_PARAMETERS
    )]
    project_root_override: Option<PathBuf>,

    /// Enable debug logging
    #[arg(long, default_value = "false", global = true, help_heading = heading::GLOBAL_PARAMETERS)]
    debug: bool,

    /// Read identity password from a file instead of prompting
    #[arg(long, global = true, value_name = "FILE", help_heading = heading::GLOBAL_PARAMETERS)]
    identity_password_file: Option<PathBuf>,

    /// Generate markdown documentation for all commands and exit
    #[arg(long, hide = true)]
    markdown_help: bool,

    #[command(subcommand)]
    command: Option<Command>,
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

    let term = TermWriter {
        debug: cli.debug,
        raw_term: Term::stdout(),
    };

    // Logging and Telemetry
    let (debug_layer, event_layer) = (
        debug_layer(), // debug
        EventLayer,    // event
    );

    let reg = Registry::default()
        .with(
            debug_layer.with_filter(
                filter::filter_fn(|_| true)
                    //
                    // Only log if `debug` is set
                    .and(filter::filter_fn(move |_| cli.debug)),
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

    // Execute the command within a span that includes version and SHA context
    let trace_span = trace_span!(
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

    let password_func: icp::identity::PasswordFunc = match cli.identity_password_file {
        Some(path) => Box::new(move || {
            icp::fs::read_to_string(&path)
                .map(|s| s.trim().to_string())
                .map_err(|e| e.to_string())
        }),
        None => Box::new(|| {
            dialoguer::Password::new()
                .with_prompt("Enter identity password")
                .interact()
                .map_err(|e| e.to_string())
        }),
    };
    let ctx = icp::context::initialize(cli.project_root_override, term, cli.debug, password_func)?;

    match command {
        // Build
        Command::Build(args) => {
            commands::build::exec(&ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Canister
        Command::Canister(cmd) => match cmd {
            commands::canister::Command::Call(args) => {
                commands::canister::call::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Create(args) => {
                commands::canister::create::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Delete(args) => {
                commands::canister::delete::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Install(args) => {
                commands::canister::install::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::List(args) => {
                commands::canister::list::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Metadata(args) => {
                commands::canister::metadata::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Settings(cmd) => match cmd {
                commands::canister::settings::Command::Show(args) => {
                    commands::canister::settings::show::exec(&ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::settings::Command::Update(args) => {
                    commands::canister::settings::update::exec(&ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::settings::Command::Sync(args) => {
                    commands::canister::settings::sync::exec(&ctx, &args)
                        .instrument(trace_span)
                        .await?
                }
            },

            commands::canister::Command::Start(args) => {
                commands::canister::start::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Status(args) => {
                commands::canister::status::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Stop(args) => {
                commands::canister::stop::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::TopUp(args) => {
                commands::canister::top_up::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Cycles
        Command::Cycles(cmd) => match cmd {
            commands::cycles::Command::Balance(args) => {
                commands::cycles::balance::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::cycles::Command::Mint(args) => {
                commands::cycles::mint::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Deploy
        Command::Deploy(args) => {
            commands::deploy::exec(&ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Environment
        Command::Environment(cmd) => match cmd {
            commands::environment::Command::List(args) => {
                commands::environment::list::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Identity
        Command::Identity(cmd) => match cmd {
            commands::identity::Command::AccountId(args) => {
                commands::identity::account_id::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Default(args) => {
                commands::identity::default::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Import(args) => {
                commands::identity::import::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::List(args) => {
                commands::identity::list::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::New(args) => {
                commands::identity::new::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Principal(args) => {
                commands::identity::principal::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Network
        Command::Network(cmd) => match cmd {
            commands::network::Command::List(args) => {
                commands::network::list::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Ping(args) => {
                commands::network::ping::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Start(args) => {
                commands::network::start::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Status(args) => {
                commands::network::status::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Stop(args) => {
                commands::network::stop::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Update(args) => {
                commands::network::update::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // New
        Command::New(args) => {
            commands::new::exec(&ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Project
        Command::Project(cmd) => match cmd {
            commands::project::Command::Show(args) => {
                commands::project::show::exec(&ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Sync
        Command::Sync(args) => {
            commands::sync::exec(&ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Token
        Command::Token(cmd) => match cmd.command {
            commands::token::Commands::Balance(args) => {
                commands::token::balance::exec(&ctx, &cmd.token, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::token::Commands::Transfer(args) => {
                commands::token::transfer::exec(&ctx, &cmd.token, &args)
                    .instrument(trace_span)
                    .await?
            }
        },
    }

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
