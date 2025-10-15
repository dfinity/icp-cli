use clap::Subcommand;

pub(crate) mod list;
pub(crate) mod ping;
pub(crate) mod run;

#[derive(Subcommand, Debug)]
pub enum Command {
    List(list::ListArgs),
    Ping(ping::PingArgs),
    Run(run::RunArgs),
}
