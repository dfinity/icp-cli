use anyhow::Context as _;
use clap::{Args, ValueHint};
use icp::context::Context;
use icp::prelude::*;

use crate::{operations::bundle::create_bundle, render::rendered};

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
    #[arg(long, short, value_hint = ValueHint::AnyPath)]
    pub(crate) output: PathBuf,

    /// Environment the canisters are built for. Bundles are made to be deployed
    /// elsewhere, so this defaults to `ic` rather than the usual `local`.
    #[arg(long, short = 'e', env = "ICP_ENVIRONMENT", default_value = IC)]
    pub(crate) environment: String,
}

pub(crate) async fn exec(ctx: &Context, args: &BundleArgs) -> Result<(), anyhow::Error> {
    let project = ctx.project.load().await.context("failed to load project")?;

    let canisters: Vec<_> = project.canisters.into_values().collect();

    let pkg_cache = ctx.dirs.package_cache()?;
    rendered(ctx.debug, async |reporter| {
        create_bundle(
            &project.dir,
            canisters,
            &args.environment,
            ctx.builder.clone(),
            ctx.artifacts.clone(),
            &pkg_cache,
            reporter,
            &args.output,
        )
        .await
    })
    .await
    .context("failed to create bundle")?;

    Ok(())
}
