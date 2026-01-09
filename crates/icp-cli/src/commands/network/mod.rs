use clap::Subcommand;

mod args;
pub(crate) mod list;
pub(crate) mod ping;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod stop;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    List(list::ListArgs),
    Ping(ping::PingArgs),
    Start(start::StartArgs),
    Status(status::StatusArgs),
    Stop(stop::Cmd),
}
