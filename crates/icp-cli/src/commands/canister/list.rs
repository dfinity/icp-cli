use std::io::stdout;

use clap::Args;
use icp::context::Context;
use serde::Serialize;

use crate::options::EnvironmentOpt;

/// List the canisters in an environment
#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
    /// Output command results as JSON
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
    let environment_selection = args.environment.clone().into();
    let env = ctx.get_environment(&environment_selection).await?;
    let canisters = env.canisters.keys().cloned().collect();
    if args.json {
        serde_json::to_writer(stdout(), &JsonList { canisters })?;
    } else {
        println!("{}", canisters.join("\n"));
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonList {
    canisters: Vec<String>,
}
