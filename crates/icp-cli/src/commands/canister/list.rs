use clap::Args;

use crate::options::EnvironmentOpt;
use icp::context::Context;

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },
}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Load target environment
    let env =
        p.environments
            .get(args.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: args.environment.name().to_owned(),
            })?;

    for (_, c) in env.canisters.values() {
        let _ = ctx.term.write_line(&format!("{c:?}"));
    }

    Ok(())
}
