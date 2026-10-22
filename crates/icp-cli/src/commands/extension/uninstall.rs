use clap::Args;
use icp_app::context::Context;
use tracing::info;

/// Uninstall an extension
#[derive(Args, Debug)]
pub(crate) struct UninstallArgs {
    /// The extension's name
    name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &UninstallArgs) -> Result<(), anyhow::Error> {
    icp_app::extension::uninstall(&ctx.dirs.extensions(), &args.name)?;
    info!("Uninstalled {}.", args.name);
    Ok(())
}
