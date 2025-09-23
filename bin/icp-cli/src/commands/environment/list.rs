use clap::Parser;
use snafu::Snafu;

use crate::context::{Context, ContextProjectError};

#[derive(Debug, Parser)]
pub struct Cmd;

pub async fn exec(ctx: &Context, _: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project()?;

    // List environments
    for e in &pm.environments {
        let _ = ctx.term.write_line(&format!("{e:?}"));
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },
}
