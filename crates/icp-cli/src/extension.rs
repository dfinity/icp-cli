//! Installed extensions as commands: each one's declared commands become an
//! ordinary subcommand of `icp`, parsed by clap like any built-in, and a
//! parsed invocation is resolved and handed to the extension to run.

use std::io::{IsTerminal, stderr, stdin, stdout};
use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use clap::builder::PossibleValuesParser;
use clap::{ArgAction, ArgMatches, Args, FromArgMatches};
use dialoguer::Confirm;
use dialoguer::console::{Term, colors_enabled_stderr};
use icp_app::context::{Context, NetworkSelection};
use icp_app::directories::{Access, Directories};
use icp_app::extension::Installed;
use icp_app::identity::IdentitySelection;
use icp_extension::{ArgKind, Matched, RunContext, StandardOpt, Terminal};
use icp_project::host::EnvironmentSelection;
use icp_project::prelude::*;
use tracing::{info, warn};

use crate::commands::cycles::buy::{black_on_white, browser_is_at_hand, render_qr};
use crate::options::{EnvironmentOpt, IdentityOpt, NetworkOpt};

/// The installed extensions, or none when the directory they live in cannot
/// be found: a broken extension store should not stop the built-in commands.
pub(crate) fn installed() -> Vec<Installed> {
    match Directories::new() {
        Ok(dirs) => icp_app::extension::list(&dirs.extensions()),
        Err(_) => vec![],
    }
}

/// `icp`'s command tree with every installed extension added to it.
pub(crate) fn cli_command(installed: &[Installed]) -> clap::Command {
    use clap::CommandFactory as _;
    let mut cli = crate::Cli::command();
    for extension in installed {
        // Install refuses a name a built-in has; this guards against a
        // built-in added since.
        if cli.find_subcommand(&extension.name).is_none() {
            cli = cli.subcommand(command(extension));
        }
    }
    cli
}

/// The clap command for one extension, built from what it declared.
fn command(installed: &Installed) -> clap::Command {
    let info = &installed.info;
    node(info, &[], installed.name.clone())
        .about(format!("{} [extension {}]", info.summary, info.version))
}

/// The command at `path`, with every declared command beneath it.
fn node(info: &icp_extension::Info, path: &[String], name: String) -> clap::Command {
    let mut cmd = clap::Command::new(name);
    // Children in the order they were first declared.
    let mut children: Vec<&String> = vec![];
    for command in &info.commands {
        if command.path.len() > path.len()
            && command.path.starts_with(path)
            && !children.contains(&&command.path[path.len()])
        {
            children.push(&command.path[path.len()]);
        }
    }
    let declared = info.commands.iter().find(|c| c.path == path);
    if let Some(declared) = declared {
        cmd = cmd.about(declared.about.clone());
        // A group takes no arguments (`validate` sees to it).
        for arg in &declared.args {
            cmd = cmd.arg(clap_arg(arg));
        }
        if declared.standard.contains(&StandardOpt::Network) {
            cmd = EnvironmentOpt::augment_args(NetworkOpt::augment_args(cmd));
        }
        if declared.standard.contains(&StandardOpt::Identity) {
            cmd = IdentityOpt::augment_args(cmd);
        }
    }
    // A group is never run itself, declared or not.
    if !children.is_empty() || declared.is_none() {
        cmd = cmd.subcommand_required(true).arg_required_else_help(true);
    }
    for child in children {
        let mut child_path = path.to_vec();
        child_path.push(child.clone());
        cmd = cmd.subcommand(node(info, &child_path, child.clone()));
    }
    cmd
}

fn clap_arg(arg: &icp_extension::Arg) -> clap::Arg {
    let mut a = clap::Arg::new(arg.name.clone()).help(arg.help.clone());
    if let Some(long) = &arg.long {
        a = a.long(long.clone());
    }
    if let Some(short) = arg.short {
        a = a.short(short);
    }
    a = match arg.kind {
        ArgKind::Flag => a.action(ArgAction::SetTrue),
        ArgKind::Value => a.action(ArgAction::Set),
        ArgKind::Values if arg.is_positional() => a.action(ArgAction::Append).num_args(1..),
        ArgKind::Values => a.action(ArgAction::Append),
    };
    if arg.kind != ArgKind::Flag {
        a = a.required(arg.required);
        a = match arg.possible_values.is_empty() {
            true => a.value_parser(clap::value_parser!(String)),
            false => a.value_parser(PossibleValuesParser::new(arg.possible_values.clone())),
        };
        if let Some(default) = &arg.default_value {
            a = a.default_value(default.clone());
        }
        if let Some(value_name) = &arg.value_name {
            a = a.value_name(value_name.clone());
        }
    }
    a
}

/// Run the extension command `matches` holds (the matches *of* the
/// extension's own subcommand).
pub(crate) async fn exec(
    ctx: &Context,
    installed: &Installed,
    matches: &ArgMatches,
) -> Result<(), anyhow::Error> {
    let info = &installed.info;

    // Down to the command that was invoked.
    let mut path = vec![];
    let mut leaf = matches;
    while let Some((name, sub)) = leaf.subcommand() {
        path.push(name.to_owned());
        leaf = sub;
    }
    let declared = info
        .commands
        .iter()
        .find(|c| c.path == path)
        .ok_or_else(|| {
            anyhow!(
                "'{}' declares no command '{}'",
                installed.name,
                path.join(" ")
            )
        })?;

    let mut values = vec![];
    for arg in &declared.args {
        match arg.kind {
            ArgKind::Flag => {
                if leaf.get_flag(&arg.name) {
                    values.push((arg.name.clone(), vec![]));
                }
            }
            ArgKind::Value | ArgKind::Values => {
                if let Some(given) = leaf.get_many::<String>(&arg.name) {
                    values.push((arg.name.clone(), given.cloned().collect()));
                }
            }
        }
    }

    // What the command asked icp-cli to resolve.
    let identity: IdentitySelection = match declared.standard.contains(&StandardOpt::Identity) {
        true => IdentityOpt::from_arg_matches(leaf)?.into(),
        false => IdentitySelection::Anonymous,
    };
    let mut run_ctx = RunContext {
        interactive: stdin().is_terminal() && stdout().is_terminal(),
        ..Default::default()
    };
    let mut calls = None;
    if declared.standard.contains(&StandardOpt::Network) {
        let mut network: NetworkSelection = NetworkOpt::from_arg_matches(leaf)?.into();
        let environment: EnvironmentSelection = EnvironmentOpt::from_arg_matches(leaf)?.into();
        if let (Some(default), NetworkSelection::Default, EnvironmentSelection::Default) =
            (&declared.default_network, &network, &environment)
        {
            network = NetworkSelection::Named(default.clone());
        }
        let agent = ctx.get_agent(&identity, &network, &environment).await?;
        run_ctx.mainnet = agent.read_root_key() == IC_ROOT_KEY.as_slice();
        match &network {
            NetworkSelection::Named(name) => run_ctx.network = Some(name.clone()),
            NetworkSelection::Url(url, _) => run_ctx.network = Some(url.to_string()),
            NetworkSelection::Default => {
                let env = ctx.host.get_environment(&environment).await?;
                run_ctx.network = Some(env.network.name.clone());
                run_ctx.environment = Some(env.name.clone());
                if let Ok(ids) = ctx.host.ids_by_environment(&environment).await {
                    run_ctx.canister_ids = ids.into_iter().collect();
                }
            }
        }
        if identity != IdentitySelection::Anonymous {
            run_ctx.identity_principal = Some(agent.get_principal().map_err(|e| anyhow!(e))?);
        }
        calls = Some(icp_app::calls::calls(agent, None)?);
    } else if identity != IdentitySelection::Anonymous {
        let id = ctx.get_identity(&identity, None).await?;
        run_ctx.identity_principal = Some(id.sender().map_err(|e| anyhow!(e))?);
    }

    let wasm = icp_app::extension::wasm(installed)?;
    icp_extension::run(
        wasm,
        info.canisters.clone(),
        Matched { path, values },
        run_ctx,
        calls,
        Arc::new(Tty),
    )
    .await
    .with_context(|| format!("extension '{}' failed", installed.name))
}

/// The terminal an extension talks to the user through.
struct Tty;

impl Terminal for Tty {
    fn confirm(&self, prompt: &str, default: bool) -> bool {
        if !(stdin().is_terminal() && stderr().is_terminal()) {
            return default;
        }
        Confirm::new()
            .with_prompt(prompt)
            .default(default)
            .interact()
            .unwrap_or(default)
    }

    fn open_url(&self, url: &str) {
        eprintln!("{url}");
        if !stderr().is_terminal() {
            return;
        }
        if browser_is_at_hand() {
            match open::that(url) {
                Ok(()) => info!("Opened it in your browser."),
                Err(e) => warn!("Could not open a browser ({e}); open the URL above yourself."),
            }
        }
        if let Some(qr) = render_qr(url) {
            let (rows, columns) = Term::stderr().size();
            let width = qr.lines().next().map_or(0, |row| row.chars().count());
            let height = qr.lines().count() + 1;
            if width <= usize::from(columns) && height <= usize::from(rows) {
                match colors_enabled_stderr() {
                    true => eprintln!("{}", black_on_white(&qr)),
                    false => eprintln!("{qr}"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use icp_extension::{AllowedCanister, Arg, Command, Info};

    use super::*;

    fn arg(name: &str, long: Option<&str>, short: Option<char>, kind: ArgKind) -> Arg {
        Arg {
            name: name.into(),
            long: long.map(Into::into),
            short,
            kind,
            required: false,
            default_value: None,
            possible_values: vec![],
            value_name: None,
            help: String::new(),
        }
    }

    fn command(path: &[&str], args: Vec<Arg>, standard: Vec<StandardOpt>) -> Command {
        Command {
            path: path.iter().map(|s| s.to_string()).collect(),
            about: "about".into(),
            args,
            standard,
            default_network: None,
        }
    }

    /// Every shape an extension can declare, valid by `validate`.
    fn every_shape() -> Installed {
        let mut required = arg("first", None, None, ArgKind::Value);
        required.required = true;
        let mut chosen = arg("chosen", Some("chosen"), Some('c'), ArgKind::Value);
        chosen.possible_values = vec!["a".into(), "b".into()];
        chosen.default_value = Some("a".into());
        chosen.value_name = Some("CHOICE".into());
        let info = Info {
            name: "demo".into(),
            version: "1.0.0".into(),
            summary: "Every shape".into(),
            commands: vec![
                command(&["group"], vec![], vec![]),
                command(
                    &["group", "leaf"],
                    vec![
                        required,
                        arg("rest", None, None, ArgKind::Values),
                        chosen,
                        arg("many", Some("many"), None, ArgKind::Values),
                        arg("yes", Some("yes"), Some('y'), ArgKind::Flag),
                    ],
                    vec![StandardOpt::Identity, StandardOpt::Network],
                ),
                command(
                    &["undeclared-parent", "leaf"],
                    vec![],
                    vec![StandardOpt::Network],
                ),
                command(&["identity-only"], vec![], vec![StandardOpt::Identity]),
            ],
            canisters: vec![AllowedCanister {
                id: candid::Principal::anonymous(),
                label: "x".into(),
            }],
        };
        icp_extension::validate(&info).unwrap();
        Installed {
            // Not the name the extension gives itself: it is the installed
            // name that is the command.
            name: "renamed".into(),
            source: String::new(),
            sha256: String::new(),
            wasm: PathBuf::new(),
            installed_at: String::new(),
            info,
        }
    }

    #[test]
    fn clap_accepts_every_declarable_shape_beside_icps_own_commands() {
        // Panics, in a debug build, on anything clap rejects.
        cli_command(&[every_shape()]).debug_assert();
    }

    #[test]
    fn a_parsed_command_reaches_its_leaf() {
        let matches = cli_command(&[every_shape()])
            .try_get_matches_from([
                "icp", "renamed", "group", "leaf", "one", "two", "three", "-c", "b", "--many", "x",
                "--many", "y", "-y", "-e", "staging",
            ])
            .unwrap();
        let (name, sub) = matches.subcommand().unwrap();
        assert_eq!(name, "renamed");
        let leaf = sub
            .subcommand_matches("group")
            .unwrap()
            .subcommand_matches("leaf")
            .unwrap();
        let strings = |id| {
            leaf.get_many::<String>(id)
                .unwrap()
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(strings("first"), ["one"]);
        assert_eq!(strings("rest"), ["two", "three"]);
        assert_eq!(strings("chosen"), ["b"]);
        assert_eq!(strings("many"), ["x", "y"]);
        assert!(leaf.get_flag("yes"));
        let environment: EnvironmentSelection =
            EnvironmentOpt::from_arg_matches(leaf).unwrap().into();
        assert_eq!(environment, EnvironmentSelection::Named("staging".into()));
    }

    #[test]
    fn a_group_needs_a_subcommand() {
        let error = cli_command(&[every_shape()])
            .try_get_matches_from(["icp", "renamed", "group"])
            .unwrap_err();
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }
}
