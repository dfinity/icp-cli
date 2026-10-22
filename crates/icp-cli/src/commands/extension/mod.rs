use clap::Subcommand;

pub(crate) mod install;
pub(crate) mod list;
pub(crate) mod uninstall;

/// Install and manage extensions: third-party commands run in a sandbox
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Install(install::InstallArgs),
    List(list::ListArgs),
    Uninstall(uninstall::UninstallArgs),
}
