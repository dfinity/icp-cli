use std::collections::HashSet;

use anyhow::Context as _;
use clap::{Args, ValueHint};
use icp::context::{Context, EnvironmentSelection};
use icp::prelude::*;
use tracing::warn;

use crate::operations::bundle::create_bundle;

/// Bundle a project into a self-contained deployable archive.
///
/// Builds the canisters the selected environment contains and packages them
/// with a rewritten manifest into a `.tar.gz` file. The rewritten manifest
/// replaces all build steps with pre-built steps referencing the bundled WASM
/// files. Asset sync directories are included in the archive.
///
/// A canister with a script sync step cannot be bundled.
#[derive(Args, Debug)]
pub(crate) struct BundleArgs {
    /// Output path for the bundle archive (e.g. bundle.tar.gz)
    #[arg(long, short, value_hint = ValueHint::AnyPath)]
    pub(crate) output: PathBuf,

    /// Environment the canisters are built for, and whose canisters the bundle
    /// carries. Bundles are made to be deployed elsewhere, so this defaults to
    /// `ic` rather than the usual `local`.
    #[arg(long, short = 'e', env = "ICP_ENVIRONMENT", default_value = IC)]
    pub(crate) environment: String,
}

pub(crate) async fn exec(ctx: &Context, args: &BundleArgs) -> Result<(), anyhow::Error> {
    let project = ctx.project.load().await.context("failed to load project")?;
    let environment_selection = EnvironmentSelection::Named(args.environment.clone());
    let env = ctx.get_environment(&environment_selection).await?;

    let canisters: Vec<_> = project.canisters.into_values().collect();
    let selected: HashSet<String> = env.canisters.keys().cloned().collect();
    if selected.is_empty() {
        warn!(
            "Environment '{}' contains no canisters; the bundle will carry none",
            args.environment
        );
    }

    create_bundle(
        &project.dir,
        canisters,
        &selected,
        &args.environment,
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
