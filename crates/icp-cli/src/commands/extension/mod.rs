use clap::Subcommand;
use icp_app::extension::Installed;
use icp_project::prelude::*;
use serde::Serialize;

pub(crate) mod install;
pub(crate) mod list;
pub(crate) mod show;
pub(crate) mod uninstall;

/// Install and manage extensions: third-party commands run in a sandbox
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Install(install::InstallArgs),
    List(list::ListArgs),
    Show(show::ShowArgs),
    Uninstall(uninstall::UninstallArgs),
}

/// An installed extension, as `show --json` and `list --json` print it.
#[derive(Serialize)]
pub(crate) struct JsonExtension<'a> {
    /// The command it is installed as.
    name: &'a str,
    /// The name the extension gives itself.
    extension: &'a str,
    version: &'a str,
    summary: &'a str,
    source: &'a str,
    sha256: &'a str,
    /// Whether the kept component still has that sha256; it is not run if not.
    intact: bool,
    wasm: &'a Path,
    installed_at: &'a str,
    /// The mainnet canisters it was approved to call.
    canisters: Vec<JsonCanister<'a>>,
    /// Its runnable commands, as typed: `icp <name> <command>`.
    commands: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct JsonCanister<'a> {
    id: String,
    label: &'a str,
}

impl<'a> JsonExtension<'a> {
    pub(crate) fn of(installed: &'a Installed) -> Self {
        let info = &installed.info;
        Self {
            name: &installed.name,
            extension: &info.name,
            version: &info.version,
            summary: &info.summary,
            source: &installed.source,
            sha256: &installed.sha256,
            intact: icp_app::extension::intact(installed),
            wasm: &installed.wasm,
            installed_at: &installed.installed_at,
            canisters: info
                .canisters
                .iter()
                .map(|c| JsonCanister {
                    id: c.id.to_text(),
                    label: &c.label,
                })
                .collect(),
            commands: runnable(installed),
        }
    }
}

/// The commands of an extension that run, rather than group others, each as
/// typed after `icp`.
pub(crate) fn runnable(installed: &Installed) -> Vec<String> {
    let commands = &installed.info.commands;
    commands
        .iter()
        .filter(|c| {
            !commands
                .iter()
                .any(|o| o.path.len() > c.path.len() && o.path.starts_with(&c.path))
        })
        .map(|c| {
            std::iter::once(installed.name.as_str())
                .chain(c.path.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}
