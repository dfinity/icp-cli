use clap::Subcommand;

pub(crate) mod list;
pub(crate) mod ping;
pub(crate) mod run;
pub(crate) mod stop;

#[derive(Subcommand, Debug)]
pub enum Command {
    List(list::ListArgs),
    Ping(ping::PingArgs),
    Run(run::RunArgs),
    Stop(stop::Cmd),
}
