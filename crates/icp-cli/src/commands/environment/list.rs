use clap::Args;

use crate::commands::Context;

#[derive(Debug, Args)]
pub struct ListArgs;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),
}

pub async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), CommandError> {
    // Load project
    let pm = ctx.project.load().await?;

    // List environments
    for e in &pm.environments {
        let _ = ctx.term.write_line(&format!("{e:?}"));
    }

    Ok(())
}
