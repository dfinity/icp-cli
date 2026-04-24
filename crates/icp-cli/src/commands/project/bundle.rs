use anyhow::Context as _;
use clap::Args;
use icp::context::Context;
use icp::prelude::*;

use crate::operations::bundle::create_bundle;

/// Bundle a project into a self-contained deployable archive.
///
/// Builds all project canisters and packages them with a rewritten manifest
/// into a `.tar.gz` file. The rewritten manifest replaces all build steps
/// with pre-built steps referencing the bundled WASM files. Asset sync
/// directories are included in the archive.
///
/// Projects with script sync steps cannot be bundled.
#[derive(Args, Debug)]
pub(crate) struct BundleArgs {
    /// Output path for the bundle archive (e.g. bundle.tar.gz)
    #[arg(long, short)]
    pub(crate) output: PathBuf,
}

pub(crate) async fn exec(ctx: &Context, args: &BundleArgs) -> Result<(), anyhow::Error> {
    let project = ctx.project.load().await.context("failed to load project")?;

    let canisters: Vec<_> = project.canisters.into_values().collect();

    create_bundle(
        &project.dir,
        canisters,
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.dirs.package_cache()?,
        ctx.debug,
        &args.output,
    )
    .await
    .context("failed to create bundle")?;

    Ok(())
}
