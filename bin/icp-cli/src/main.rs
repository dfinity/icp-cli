use crate::commands::Cli;
use clap::Parser;

mod commands;
mod project;

#[tokio::main]
async fn main() {
    commands::dispatch(Cli::parse()).await;
}
