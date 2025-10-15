use clap::Parser;

use crate::commands::Context;

/// List networks in the project
#[derive(Parser, Debug)]
pub struct Cmd;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub async fn exec(ctx: &Context, _: Cmd) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // List networks
    for (name, cfg) in &p.networks {
        tracing::info!("{name} => {cfg:?}");
    }

    Ok(())
}
