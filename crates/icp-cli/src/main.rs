use anyhow::Error;
use clap::{CommandFactory, Parser};
use commands::Command;
use console::Term;
use icp::{context::TermWriter, prelude::*, settings::Settings};
use tracing::{Instrument, debug, subscriber::set_global_default, trace_span};
use tracing_subscriber::{
    Layer, Registry,
    filter::{self, FilterExt},
    layer::SubscriberExt,
};

use crate::{
    logging::debug_layer,
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
    // -----------------------------------------------------------------------
    // Hidden telemetry-send-batch mode: spawned as a detached child process.
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

    let term = TermWriter {
        debug: cli.debug,
        raw_term: Term::stdout(),
    };

    // Logging
    let debug_layer = debug_layer();

    let reg = Registry::default().with(
        debug_layer
            .with_filter(filter::filter_fn(|_| true).and(filter::filter_fn(move |_| cli.debug))),
    );

    // Set the configured subscriber registry as the global default for tracing
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

    // -----------------------------------------------------------------------
    // Telemetry setup
    // -----------------------------------------------------------------------
    let telemetry_session = setup_telemetry(&ctx, &command, &raw_args).await;

    // -----------------------------------------------------------------------
    // Command dispatch
    // -----------------------------------------------------------------------
    let result = dispatch(&ctx, command, trace_span).await;

    // -----------------------------------------------------------------------
    // Record the telemetry event
    // -----------------------------------------------------------------------
    if let Some(session) = telemetry_session {
        session.finish(result.is_ok());
    }

    result?;

    debug!("Command executed successfully");

    Ok(())
}

/// Initialise a telemetry session unless telemetry is disabled.
async fn setup_telemetry(
    ctx: &icp::context::Context,
    command: &Command,
    raw_args: &[String],
) -> Option<telemetry::TelemetrySession> {
    if telemetry::is_disabled_by_env() {
        return None;
    }

    let telemetry_dir = ctx.dirs.telemetry_data();

    // Load settings to check the user preference (best-effort; default to enabled)
    let enabled = async {
        let dirs = ctx.dirs.settings().ok()?;
        let settings = dirs
            .with_read(async |dirs| Settings::load_from(dirs))
            .await
            .ok()?
            .ok()?;
        Some(settings.telemetry_enabled)
    }
    .await
    .unwrap_or(true);

    if !enabled {
        return None;
    }

    telemetry::show_notice_if_needed(&telemetry_dir);

    let cmd_name = command_telemetry_name(command).to_string();
    let flags = telemetry::collect_flags(raw_args);
    let version = icp_cli_version_str().to_string();

    Some(telemetry::TelemetrySession::begin(
        telemetry_dir,
        cmd_name,
        flags,
        version,
    ))
}

/// Map a parsed `Command` to its telemetry name string.
///
/// This is an exhaustive match rather than a runtime string extraction from
/// argv so that adding a new `Command` variant causes a compile error here,
/// forcing the author to assign an explicit telemetry name.
///
/// Deriving the name automatically from argv would risk leaking positional
/// argument values (e.g. project names) into telemetry and would be fragile when
/// flags appear before subcommands.
fn command_telemetry_name(cmd: &Command) -> &'static str {
    use commands::{canister, cycles, environment, identity, network, project, token};
    match cmd {
        Command::Build(_) => "build",
        Command::Deploy(_) => "deploy",
        Command::New(_) => "new",
        Command::Sync(_) => "sync",
        Command::Settings(_) => "settings",

        Command::Canister(sub) => match sub {
            canister::Command::Call(_) => "canister call",
            canister::Command::Create(_) => "canister create",
            canister::Command::Delete(_) => "canister delete",
            canister::Command::Install(_) => "canister install",
            canister::Command::List(_) => "canister list",
            canister::Command::Logs(_) => "canister logs",
            canister::Command::Metadata(_) => "canister metadata",
            canister::Command::MigrateId(_) => "canister migrate-id",
            canister::Command::Settings(sub) => match sub {
                canister::settings::Command::Show(_) => "canister settings show",
                canister::settings::Command::Update(_) => "canister settings update",
                canister::settings::Command::Sync(_) => "canister settings sync",
            },
            canister::Command::Snapshot(sub) => match sub {
                canister::snapshot::Command::Create(_) => "canister snapshot create",
                canister::snapshot::Command::Delete(_) => "canister snapshot delete",
                canister::snapshot::Command::Download(_) => "canister snapshot download",
                canister::snapshot::Command::List(_) => "canister snapshot list",
                canister::snapshot::Command::Restore(_) => "canister snapshot restore",
                canister::snapshot::Command::Upload(_) => "canister snapshot upload",
            },
            canister::Command::Start(_) => "canister start",
            canister::Command::Status(_) => "canister status",
            canister::Command::Stop(_) => "canister stop",
            canister::Command::TopUp(_) => "canister top-up",
        },

        Command::Cycles(sub) => match sub {
            cycles::Command::Balance(_) => "cycles balance",
            cycles::Command::Mint(_) => "cycles mint",
            cycles::Command::Transfer(_) => "cycles transfer",
        },

        Command::Environment(sub) => match sub {
            environment::Command::List(_) => "environment list",
        },

        Command::Identity(sub) => match sub {
            identity::Command::AccountId(_) => "identity account-id",
            identity::Command::Default(_) => "identity default",
            identity::Command::Delete(_) => "identity delete",
            identity::Command::Export(_) => "identity export",
            identity::Command::Import(_) => "identity import",
            identity::Command::Link(sub) => match sub {
                identity::link::Command::Hsm(_) => "identity link hsm",
            },
            identity::Command::List(_) => "identity list",
            identity::Command::New(_) => "identity new",
            identity::Command::Principal(_) => "identity principal",
            identity::Command::Rename(_) => "identity rename",
        },

        Command::Network(sub) => match sub {
            network::Command::List(_) => "network list",
            network::Command::Ping(_) => "network ping",
            network::Command::Start(_) => "network start",
            network::Command::Status(_) => "network status",
            network::Command::Stop(_) => "network stop",
            network::Command::Update(_) => "network update",
        },

        Command::Project(sub) => match sub {
            project::Command::Show(_) => "project show",
        },

        Command::Token(sub) => match sub.command {
            token::Commands::Balance(_) => "token balance",
            token::Commands::Transfer(_) => "token transfer",
        },
    }
}

/// Dispatch the command to its handler.
async fn dispatch(
    ctx: &icp::context::Context,
    command: Command,
    trace_span: tracing::Span,
) -> Result<(), Error> {
    match command {
        // Build
        Command::Build(args) => {
            commands::build::exec(ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Canister
        Command::Canister(cmd) => match cmd {
            commands::canister::Command::Call(args) => {
                commands::canister::call::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Create(args) => {
                commands::canister::create::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Delete(args) => {
                commands::canister::delete::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Install(args) => {
                commands::canister::install::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::List(args) => {
                commands::canister::list::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Logs(args) => {
                commands::canister::logs::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Metadata(args) => {
                commands::canister::metadata::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::MigrateId(args) => {
                commands::canister::migrate_id::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Settings(cmd) => match cmd {
                commands::canister::settings::Command::Show(args) => {
                    commands::canister::settings::show::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::settings::Command::Update(args) => {
                    commands::canister::settings::update::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::settings::Command::Sync(args) => {
                    commands::canister::settings::sync::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }
            },

            commands::canister::Command::Snapshot(cmd) => match cmd {
                commands::canister::snapshot::Command::Create(args) => {
                    commands::canister::snapshot::create::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::snapshot::Command::Delete(args) => {
                    commands::canister::snapshot::delete::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::snapshot::Command::Download(args) => {
                    commands::canister::snapshot::download::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::snapshot::Command::List(args) => {
                    commands::canister::snapshot::list::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::snapshot::Command::Restore(args) => {
                    commands::canister::snapshot::restore::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }

                commands::canister::snapshot::Command::Upload(args) => {
                    commands::canister::snapshot::upload::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }
            },

            commands::canister::Command::Start(args) => {
                commands::canister::start::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Status(args) => {
                commands::canister::status::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::Stop(args) => {
                commands::canister::stop::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::canister::Command::TopUp(args) => {
                commands::canister::top_up::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Cycles
        Command::Cycles(cmd) => match cmd {
            commands::cycles::Command::Balance(args) => {
                commands::cycles::balance::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::cycles::Command::Mint(args) => {
                commands::cycles::mint::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::cycles::Command::Transfer(args) => {
                commands::cycles::transfer::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Deploy
        Command::Deploy(args) => {
            commands::deploy::exec(ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Environment
        Command::Environment(cmd) => match cmd {
            commands::environment::Command::List(args) => {
                commands::environment::list::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Identity
        Command::Identity(cmd) => match cmd {
            commands::identity::Command::AccountId(args) => {
                commands::identity::account_id::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Default(args) => {
                commands::identity::default::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Delete(args) => {
                commands::identity::delete::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Export(args) => {
                commands::identity::export::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Import(args) => {
                commands::identity::import::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Link(cmd) => match cmd {
                commands::identity::link::Command::Hsm(args) => {
                    commands::identity::link::hsm::exec(ctx, &args)
                        .instrument(trace_span)
                        .await?
                }
            },

            commands::identity::Command::List(args) => {
                commands::identity::list::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::New(args) => {
                commands::identity::new::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Principal(args) => {
                commands::identity::principal::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::identity::Command::Rename(args) => {
                commands::identity::rename::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Network
        Command::Network(cmd) => match cmd {
            commands::network::Command::List(args) => {
                commands::network::list::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Ping(args) => {
                commands::network::ping::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Start(args) => {
                commands::network::start::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Status(args) => {
                commands::network::status::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Stop(args) => {
                commands::network::stop::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::network::Command::Update(args) => {
                commands::network::update::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // New
        Command::New(args) => {
            commands::new::exec(ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Project
        Command::Project(cmd) => match cmd {
            commands::project::Command::Show(args) => {
                commands::project::show::exec(ctx, &args)
                    .instrument(trace_span)
                    .await?
            }
        },

        // Settings
        Command::Settings(args) => {
            commands::settings::exec(ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Sync
        Command::Sync(args) => {
            commands::sync::exec(ctx, &args)
                .instrument(trace_span)
                .await?
        }

        // Token
        Command::Token(cmd) => match cmd.command {
            commands::token::Commands::Balance(args) => {
                commands::token::balance::exec(ctx, &cmd.token_name_or_ledger_id, &args)
                    .instrument(trace_span)
                    .await?
            }

            commands::token::Commands::Transfer(args) => {
                commands::token::transfer::exec(ctx, &cmd.token_name_or_ledger_id, &args)
                    .instrument(trace_span)
                    .await?
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
