use clap::Args;
use icp::context::Context;

use crate::options::EnvironmentOpt;

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
    let environment_selection = args.environment.clone().into();
    let env = ctx.get_environment(&environment_selection).await?;

    for (_, c) in env.canisters.values() {
        let _ = ctx.term.write_line(&format!("{c:?}"));
    }

    Ok(())
}
