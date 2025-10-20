use clap::Args;

use crate::commands::{Context, Mode, args};

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[arg(long)]
    pub(crate) environment: Option<args::Environment>,
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

            // Argument (Environment)
            let args::Environment::Name(env) = args.environment.clone().unwrap_or_default();

            // Load target environment
            let env = p
                .environments
                .get(&env)
                .ok_or(CommandError::EnvironmentNotFound { name: env })?;

            for (_, c) in env.canisters.values() {
                let _ = ctx.term.write_line(&format!("{c:?}"));
            }
        }
    }

    Ok(())
}
