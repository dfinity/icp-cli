use anyhow::Context as _;
use clap::Parser;

use crate::context::Context;

/// List networks in the project
#[derive(Parser, Debug)]
pub struct Cmd;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub async fn exec(ctx: &Context, _: Cmd) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await.context("failed to load project")?;

    // List networks
    for (name, cfg) in &p.networks {
        eprintln!("{name} => {cfg:?}");
    }

    Ok(())
}
