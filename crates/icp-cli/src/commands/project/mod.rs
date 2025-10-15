use anyhow::Context as _;
use clap::{Parser, Subcommand};

use crate::commands::Context;

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: ProjectSubcmd,
}

#[derive(Debug, Subcommand)]
pub enum ProjectSubcmd {
    /// Outputs the project's effective yaml configuration.
    Show(ShowCmd),
}

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        ProjectSubcmd::Show(subcmd) => show(ctx, subcmd).await?,
    }
    Ok(())
}

#[derive(Parser, Debug)]
pub struct ShowCmd;

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Loads the project's configuration and output the effective yaml config
/// after resolving recipes
async fn show(ctx: &Context, _: ShowCmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await.context("failed to load project")?;

    let yaml = serde_yaml::to_string(&p).expect("Serializing to yaml failed");
    println!("{yaml}");

    Ok(())
}
