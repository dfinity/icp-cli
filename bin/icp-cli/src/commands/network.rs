use crate::commands::{Cli, network};
use clap::{Parser, Subcommand};

mod run;
mod start;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Run(run::Cmd),
    Start(start::Cmd),
}

pub async fn exec(cmd: Cmd) {
    match cmd.subcmd {
        Subcmd::Run(cmd) => run::exec(cmd).await,
        Subcmd::Start(cmd) => todo!(), // start::exec(cmd).await,
    }
}
