use clap::Args;

use icp::context::Context;

/// List networks in the project
#[derive(Args, Debug)]
pub(crate) struct ListArgs;

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), anyhow::Error> {
    // Load project
    let p = ctx.project.load().await?;

    // List networks
    for (name, cfg) in &p.networks {
        eprintln!("{name} => {cfg:?}");
    }

    Ok(())
}
