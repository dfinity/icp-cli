use clap::Args;
use icp::context::Context;

use crate::options::IdentityOpt;

#[derive(Debug, Args)]
pub(crate) struct PrincipalArgs {
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &PrincipalArgs) -> Result<(), anyhow::Error> {
    let id = ctx.get_identity(&args.identity.clone().into()).await?;

    let principal = id
        .sender()
        .map_err(|_| anyhow::anyhow!("failed to load identity principal"))?;

    println!("{principal}");

    Ok(())
}
