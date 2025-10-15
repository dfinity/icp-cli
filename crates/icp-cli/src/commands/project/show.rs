use anyhow::Context as _;
use clap::Parser;

use crate::commands::Context;

#[derive(Parser, Debug)]
pub struct ShowCmd;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Loads the project's configuration and output the effective yaml config
/// after resolving recipes
pub async fn exec(ctx: &Context, _: ShowCmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await.context("failed to load project")?;

    let yaml = serde_yaml::to_string(&p).expect("Serializing to yaml failed");
    println!("{yaml}");

    Ok(())
}
