use anyhow::Context as _;
use clap::Args;

use crate::commands::{Context, Mode};

#[derive(Args, Debug)]
pub(crate) struct ShowArgs;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Loads the project's configuration and output the effective yaml config
/// after resolving recipes
pub(crate) async fn exec(ctx: &Context, _: &ShowArgs) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await.context("failed to load project")?;

    let yaml = serde_yaml::to_string(&p).expect("Serializing to yaml failed");
    println!("{yaml}");

    Ok(())
}
