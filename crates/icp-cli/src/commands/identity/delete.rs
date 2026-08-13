use crate::context::Context;
use crate::identity::key::delete_identity;
use clap::Args;
use clap_complete::ArgValueCandidates;
use tracing::info;

/// Delete an identity
#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    /// Name of the identity to delete
    #[arg(add = ArgValueCandidates::new(crate::complete::identity_names))]
    name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.identity_dirs()?;

    dirs.with_write(async |dirs| {
        delete_identity(dirs, &args.name)?;
        info!("Deleted identity `{}`", args.name);
        Ok(())
    })
    .await?
}
