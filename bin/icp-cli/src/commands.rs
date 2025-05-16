use clap::{Parser, Subcommand};

mod network;

#[derive(Parser, Debug)]
pub struct Cli {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Network(network::Cmd),
}

pub async fn exec(cli: Cli) {
    match cli.subcommand {
        Subcmd::Network(opts) => network::exec(opts).await,
    }
}
