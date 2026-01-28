use clap::Args;
use icp::context::Context;
use icp::identity::key::rename_identity;

#[derive(Debug, Args)]
pub(crate) struct RenameArgs {
    /// Current name of the identity
    old_name: String,

    /// New name for the identity
    new_name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &RenameArgs) -> Result<(), anyhow::Error> {
    let dirs = ctx.dirs.identity()?;

    dirs.with_write(async |dirs| {
        rename_identity(dirs, &args.old_name, &args.new_name)?;
        let _ = ctx.term.write_line(&format!(
            "Renamed identity `{}` to `{}`",
            args.old_name, args.new_name
        ));
        Ok(())
    })
    .await?
}
