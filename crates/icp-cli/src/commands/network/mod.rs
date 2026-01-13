use clap::Subcommand;

mod args;
pub(crate) mod list;
pub(crate) mod ping;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;
pub(crate) mod update;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// List available networks in the project.
    List(list::ListArgs),
    /// Ping a network for liveness.
    Ping(ping::PingArgs),
    /// Start a new project-local network.
    Start(start::StartArgs),
    /// Show the status of a running network.
    Status(status::StatusArgs),
    /// Stop a network started with `icp network start --background`.
    Stop(stop::Cmd),
    /// Update the network launcher to the latest version.
    Update(update::UpdateArgs),
}
