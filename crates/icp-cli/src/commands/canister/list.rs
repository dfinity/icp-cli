use clap::Args;

use crate::{
    commands::{Context, Mode},
    options::EnvironmentOpt,
};

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
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(pdir) => {
            // Load project
            let p = ctx.project.load(pdir).await?;

            // Load target environment
            let env = p.environments.get(args.environment.name()).ok_or(
                CommandError::EnvironmentNotFound {
                    name: args.environment.name().to_owned(),
                },
            )?;

            for (_, c) in env.canisters.values() {
                let _ = ctx.term.write_line(&format!("{c:?}"));
            }
        }
    }

    Ok(())
}
