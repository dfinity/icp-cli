use clap::Args;
use icp::context::Context;

/// List the environments defined in this project, one per line.
///
/// Use `icp project show` to see the fully expanded configuration including
/// implicit environments (local, ic) and their network and canister assignments.
#[derive(Args, Debug)]
pub(crate) struct ListArgs;

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), anyhow::Error> {
    // Load project
    let pm = ctx.project.load().await?;

    for e in pm.environments.keys() {
        println!("{e}");
    }

    Ok(())
}
