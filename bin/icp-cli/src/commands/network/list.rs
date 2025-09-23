use clap::Parser;
use snafu::Snafu;

use crate::context::{Context, ContextProjectError};

/// List networks in the project
#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn exec(ctx: &Context, _: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project()?;

    // List networks
    for (name, cfg) in &pm.networks {
        eprintln!("{name} => {cfg:?}");
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },
}
