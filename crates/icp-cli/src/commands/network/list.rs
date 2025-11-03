use clap::Args;

use icp::context::Context;

/// List networks in the project
#[derive(Args, Debug)]
pub(crate) struct ListArgs;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // List networks
    for (name, cfg) in &p.networks {
        eprintln!("{name} => {cfg:?}");
    }

    Ok(())
}
