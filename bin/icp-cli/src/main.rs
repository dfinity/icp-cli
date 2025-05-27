use crate::commands::Cli;
use clap::Parser;

mod commands;
mod project;

#[tokio::main]
async fn main() {
    if let Err(e) = commands::dispatch(Cli::parse()).await {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
