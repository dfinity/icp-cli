use clap::Args;
use icp::context::Context;

#[derive(Args, Debug)]
pub(crate) struct ListArgs;

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), anyhow::Error> {
    // Load project
    let pm = ctx.project.load().await?;

    for e in pm.networks.keys() {
        ctx.term.write_line(e)?;
    }

    Ok(())
}
