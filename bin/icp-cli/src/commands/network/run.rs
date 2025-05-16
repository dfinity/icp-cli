use crate::project::structure::ProjectStructure;
use clap::Parser;

#[derive(Parser, Debug)]
pub struct Cmd {}

pub async fn exec(cmd: Cmd) {
    println!("Running network command");
}
