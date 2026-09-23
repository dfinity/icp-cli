use clap::Args;
use clap_complete::ArgValueCandidates;
use futures::future::try_join_all;
use icp_app::context::Context;
use icp_project::host::EnvironmentSelection;

use tracing::info;

use icp_project::operations::build::build_many;

use crate::{
    options::{EnvironmentOpt, arg_struct_change_help},
    render::rendered,
};

/// Build canisters
#[derive(Debug, Args)]
pub(crate) struct BuildArgs {
    /// Canister names (if empty, build all canisters in environment)
    #[arg(add = ArgValueCandidates::new(crate::complete::canisters))]
    pub(crate) canisters: Vec<String>,

    #[command(flatten)]
    pub(crate) environment: BuildEnvironmentOpt,
}

arg_struct_change_help!(
    EnvironmentOpt => BuildEnvironmentOpt,
    arg = "environment",
    help = "Override the environment to build for. By default, the local environment is used"
);

pub(crate) async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), anyhow::Error> {
    // Get environment selection
    let environment_selection: EnvironmentSelection = args.environment.0.clone().into();

    // Load target environment
    let env = ctx.host.get_environment(&environment_selection).await?;

    // Determine which canisters to build
    let cnames = match args.canisters.is_empty() {
        // No canisters specified - build all in environment
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.canisters.clone(),
    };

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    let canisters_to_build = try_join_all(cnames.iter().map(|name| {
        ctx.host
            .get_canister_and_path_for_env(name, &environment_selection)
    }))
    .await?;
    // Build the selected canisters
    info!("Building canisters:");

    rendered(ctx.debug, async |reporter| {
        build_many(
            canisters_to_build,
            environment_selection.name(),
            ctx.host.builder.clone(),
            ctx.host.artifacts.clone(),
            ctx.host.files.as_ref(),
            reporter,
        )
        .await
    })
    .await?;

    info!("Canisters built successfully");

    Ok(())
}
