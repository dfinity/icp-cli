use anyhow::Error;
use clap::{CommandFactory, Parser};
use commands::Command;
use icp::prelude::*;
use tracing::{Instrument, debug, info, subscriber::set_global_default, trace_span};
use tracing_subscriber::{Registry, layer::SubscriberExt};

use crate::{
    dist::dist_update_suggestion,
    logging::{UserLayer, debug_layer},
    operations::update_check::update_check,
    version::{git_sha, icp_cli_version_str},
};

mod artifacts;
mod commands;
mod dist;
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
    // -----------------------------------------------------------------------
    // Background telemetry-send-batch mode: spawned as a detached child process.
    // Handle it before any other setup and exit immediately after.
    // -----------------------------------------------------------------------
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.get(1).map(String::as_str) == Some("__telemetry-send-batch") {
        if let Some(batch_path) = raw_args.get(2) {
            telemetry::handle_send_batch(batch_path).await;
        }
        return Ok(());
    }

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

    // Logging: --debug gets the detailed tracing layer; otherwise plain user-facing output
    let debug = cli.debug;
    let reg = Registry::default()
        .with(debug.then(debug_layer))
        .with((!debug).then(UserLayer::new));
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
    let ctx = icp::context::initialize(cli.project_root_override, cli.debug, password_func)?;

    let telemetry_session = telemetry::setup(&ctx, &raw_args, &Cli::command()).await;

    // Run update check in the background
    let update_check = tokio::spawn({
        let ctx = ctx.clone();
        async move { update_check(&ctx).await }
    });

    let result = dispatch(&ctx, command).instrument(trace_span).await;

    if let Some(session) = telemetry_session {
        session.finish(result.is_ok(), &ctx.telemetry_data);
    }

    // Show update nag after command output
    if let Some(latest) = update_check.await.unwrap_or(None) {
        let suggestion = dist_update_suggestion(&latest)
            .unwrap_or("See https://github.com/dfinity/icp-cli/releases for upgrade instructions.");
        info!(
            "A newer version of icp-cli is available: {latest} (current: {current}). {suggestion}",
            current = icp_cli_version_str(),
        );
    }

    result?;

    debug!("Command executed successfully");

    Ok(())
}

/// Dispatch the command to its handler.
async fn dispatch(ctx: &icp::context::Context, command: Command) -> Result<(), Error> {
    match command {
        // Build
        Command::Build(args) => commands::build::exec(ctx, &args).await?,

        // Canister
        Command::Canister(cmd) => match cmd {
            commands::canister::Command::Call(args) => {
                commands::canister::call::exec(ctx, &args).await?
            }

            commands::canister::Command::Create(args) => {
                commands::canister::create::exec(ctx, &args).await?
            }

            commands::canister::Command::Delete(args) => {
                commands::canister::delete::exec(ctx, &args).await?
            }

            commands::canister::Command::Install(args) => {
                commands::canister::install::exec(ctx, &args).await?
            }

            commands::canister::Command::List(args) => {
                commands::canister::list::exec(ctx, &args).await?
            }

            commands::canister::Command::Logs(args) => {
                commands::canister::logs::exec(ctx, &args).await?
            }

            commands::canister::Command::Metadata(args) => {
                commands::canister::metadata::exec(ctx, &args).await?
            }

            commands::canister::Command::MigrateId(args) => {
                commands::canister::migrate_id::exec(ctx, &args).await?
            }

            commands::canister::Command::Settings(cmd) => match cmd {
                commands::canister::settings::Command::Show(args) => {
                    commands::canister::settings::show::exec(ctx, &args).await?
                }

                commands::canister::settings::Command::Update(args) => {
                    commands::canister::settings::update::exec(ctx, &args).await?
                }

                commands::canister::settings::Command::Sync(args) => {
                    commands::canister::settings::sync::exec(ctx, &args).await?
                }
            },

            commands::canister::Command::Snapshot(cmd) => match cmd {
                commands::canister::snapshot::Command::Create(args) => {
                    commands::canister::snapshot::create::exec(ctx, &args).await?
                }

                commands::canister::snapshot::Command::Delete(args) => {
                    commands::canister::snapshot::delete::exec(ctx, &args).await?
                }

                commands::canister::snapshot::Command::Download(args) => {
                    commands::canister::snapshot::download::exec(ctx, &args).await?
                }

                commands::canister::snapshot::Command::List(args) => {
                    commands::canister::snapshot::list::exec(ctx, &args).await?
                }

                commands::canister::snapshot::Command::Restore(args) => {
                    commands::canister::snapshot::restore::exec(ctx, &args).await?
                }

                commands::canister::snapshot::Command::Upload(args) => {
                    commands::canister::snapshot::upload::exec(ctx, &args).await?
                }
            },

            commands::canister::Command::Start(args) => {
                commands::canister::start::exec(ctx, &args).await?
            }

            commands::canister::Command::Status(args) => {
                commands::canister::status::exec(ctx, &args).await?
            }

            commands::canister::Command::Stop(args) => {
                commands::canister::stop::exec(ctx, &args).await?
            }

            commands::canister::Command::TopUp(args) => {
                commands::canister::top_up::exec(ctx, &args).await?
            }
        },

        // Cycles
        Command::Cycles(cmd) => match cmd {
            commands::cycles::Command::Balance(args) => {
                commands::cycles::balance::exec(ctx, &args).await?
            }

            commands::cycles::Command::Mint(args) => {
                commands::cycles::mint::exec(ctx, &args).await?
            }

            commands::cycles::Command::Transfer(args) => {
                commands::cycles::transfer::exec(ctx, &args).await?
            }
        },

        // Deploy
        Command::Deploy(args) => commands::deploy::exec(ctx, &args).await?,

        // Environment
        Command::Environment(cmd) => match cmd {
            commands::environment::Command::List(args) => {
                commands::environment::list::exec(ctx, &args).await?
            }
        },

        // Identity
        Command::Identity(cmd) => match cmd {
            commands::identity::Command::AccountId(args) => {
                commands::identity::account_id::exec(ctx, &args).await?
            }

            commands::identity::Command::Default(args) => {
                commands::identity::default::exec(ctx, &args).await?
            }

            commands::identity::Command::Delegation(cmd) => match cmd {
                commands::identity::delegation::Command::Request(args) => {
                    commands::identity::delegation::request::exec(ctx, &args).await?
                }
                commands::identity::delegation::Command::Sign(args) => {
                    commands::identity::delegation::sign::exec(ctx, &args).await?
                }
                commands::identity::delegation::Command::Use(args) => {
                    commands::identity::delegation::r#use::exec(ctx, &args).await?
                }
            },

            commands::identity::Command::Delete(args) => {
                commands::identity::delete::exec(ctx, &args).await?
            }

            commands::identity::Command::Export(args) => {
                commands::identity::export::exec(ctx, &args).await?
            }

            commands::identity::Command::Import(args) => {
                commands::identity::import::exec(ctx, &args).await?
            }

            commands::identity::Command::Link(cmd) => match cmd {
                commands::identity::link::Command::Hsm(args) => {
                    commands::identity::link::hsm::exec(ctx, &args).await?
                }
                commands::identity::link::Command::Ii(args) => {
                    commands::identity::link::ii::exec(ctx, &args).await?
                }
            },

            commands::identity::Command::List(args) => {
                commands::identity::list::exec(ctx, &args).await?
            }

            commands::identity::Command::New(args) => {
                commands::identity::new::exec(ctx, &args).await?
            }

            commands::identity::Command::Principal(args) => {
                commands::identity::principal::exec(ctx, &args).await?
            }

            commands::identity::Command::Login(args) => {
                commands::identity::login::exec(ctx, &args).await?
            }

            commands::identity::Command::Rename(args) => {
                commands::identity::rename::exec(ctx, &args).await?
            }
        },

        // Network
        Command::Network(cmd) => match cmd {
            commands::network::Command::List(args) => {
                commands::network::list::exec(ctx, &args).await?
            }

            commands::network::Command::Ping(args) => {
                commands::network::ping::exec(ctx, &args).await?
            }

            commands::network::Command::Start(args) => {
                commands::network::start::exec(ctx, &args).await?
            }

            commands::network::Command::Status(args) => {
                commands::network::status::exec(ctx, &args).await?
            }

            commands::network::Command::Stop(args) => {
                commands::network::stop::exec(ctx, &args).await?
            }

            commands::network::Command::Update(args) => {
                commands::network::update::exec(ctx, &args).await?
            }
        },

        // New
        Command::New(args) => commands::new::exec(ctx, &args).await?,

        // Project
        Command::Project(cmd) => match cmd {
            commands::project::Command::Show(args) => {
                commands::project::show::exec(ctx, &args).await?
            }
            commands::project::Command::Bundle(args) => {
                commands::project::bundle::exec(ctx, &args).await?
            }
        },

        // Settings
        Command::Settings(args) => commands::settings::exec(ctx, &args).await?,

        // Sync
        Command::Sync(args) => commands::sync::exec(ctx, &args).await?,

        // Token
        Command::Token(cmd) => match cmd.command {
            commands::token::Commands::Balance(args) => {
                commands::token::balance::exec(ctx, &cmd.token_name_or_ledger_id, &args).await?
            }

            commands::token::Commands::Transfer(args) => {
                commands::token::transfer::exec(ctx, &cmd.token_name_or_ledger_id, &args).await?
            }
        },
    }

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
