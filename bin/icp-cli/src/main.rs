use crate::commands::Cli;
use crate::project::structure::ProjectStructure;
use clap::Parser;
use directories::ProjectDirs;

mod commands;
mod project;

#[tokio::main]
async fn main() {
    println!("Hello, world!");
    let ps = ProjectStructure::find();

    commands::exec(Cli::parse()).await;
}
