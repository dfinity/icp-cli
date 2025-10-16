use clap::Args;

use crate::{
    commands::{Context, Mode},
    options::EnvironmentOpt,
};

#[derive(Debug, Args)]
pub struct ListArgs {
    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },
}

pub async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            // Load project
            let p = ctx.project.load().await?;

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
