use clap::{Parser, Subcommand};

pub(crate) mod list;
pub(crate) mod ping;
pub(crate) mod run;
pub(crate) mod stop;

#[derive(Parser, Debug)]
pub struct NetworkCmd {
    #[command(subcommand)]
    subcmd: NetworkSubcmd,
}

#[derive(Subcommand, Debug)]
pub enum NetworkSubcmd {
    List(list::ListArgs),
    Ping(ping::PingArgs),
    Run(run::RunArgs),
    Stop(stop::Cmd),
}
