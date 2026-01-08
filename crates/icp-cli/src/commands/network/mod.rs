use clap::Subcommand;

pub(crate) mod info;
pub(crate) mod list;
pub(crate) mod ping;
pub(crate) mod start;
pub(crate) mod stop;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Info(info::InfoArgs),
    List(list::ListArgs),
    Ping(ping::PingArgs),
    Start(start::StartArgs),
    Stop(stop::Cmd),
}
