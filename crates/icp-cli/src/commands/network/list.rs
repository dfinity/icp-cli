use clap::Args;

use crate::commands::Context;

/// List networks in the project
#[derive(Args, Debug)]
pub struct ListArgs;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // List networks
    for (name, cfg) in &p.networks {
        eprintln!("{name} => {cfg:?}");
    }

    Ok(())
}
