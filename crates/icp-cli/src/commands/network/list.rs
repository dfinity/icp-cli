use clap::Args;
use icp_app::context::Context;

/// List all networks configured in the project
#[derive(Args, Debug)]
pub(crate) struct ListArgs;

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), anyhow::Error> {
    // Load project
    let pm = ctx.host.project.load().await?;

    for e in pm.networks.keys() {
        println!("{e}");
    }

    Ok(())
}
