use clap::Subcommand;

pub(crate) mod list;
pub(crate) mod ping;
pub(crate) mod run;

#[derive(Subcommand, Debug)]
pub enum Command {
    List(list::Cmd),
    Ping(ping::Cmd),
    Run(run::Cmd),
}
