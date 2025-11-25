use clap::Args;
use icp::context::Context;

#[derive(Debug, Args)]
pub(crate) struct ListArgs;

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), anyhow::Error> {
    // Load project
    let pm = ctx.project.load().await?;

    // List environments
    for e in &pm.environments {
        let _ = ctx.term.write_line(&format!("{e:?}"));
    }

    Ok(())
}
