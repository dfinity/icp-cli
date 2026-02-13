use clap::Args;
use icp::context::Context;
use icp::identity::key::delete_identity;

/// Delete an identity
#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    /// Name of the identity to delete
    name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.identity()?;

    dirs.with_write(async |dirs| {
        delete_identity(dirs, &args.name)?;
        let _ = ctx
            .term
            .write_line(&format!("Deleted identity `{}`", args.name));
        Ok(())
    })
    .await?
}
