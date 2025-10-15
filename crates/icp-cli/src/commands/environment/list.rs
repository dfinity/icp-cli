use clap::Parser;

use crate::commands::Context;

#[derive(Debug, Parser)]
pub struct Cmd;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),
}

pub async fn exec(ctx: &Context, _: Cmd) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project.load().await?;

    // List environments
    for e in &pm.environments {
        ctx.println(&format!("{e:?}"));
    }

    Ok(())
}
