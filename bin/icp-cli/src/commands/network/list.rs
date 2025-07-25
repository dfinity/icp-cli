use clap::Parser;
use snafu::Snafu;

use crate::context::{Context, GetProjectError};

/// List networks in the project
#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn exec(ctx: &Context, _: Cmd) -> Result<(), CommandError> {
    // Load project
    let project = ctx.project()?;

    for (name, cfg) in &project.networks {
        eprintln!("{name} => {cfg:?}");
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },
}
