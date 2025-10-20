use clap::Args;

use crate::commands::{Context, Mode};

#[derive(Debug, Args)]
pub(crate) struct ListArgs;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),
}

pub(crate) async fn exec(ctx: &Context, _: &ListArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            // Load project
            let pm = ctx.project.load().await?;

            // List environments
            for e in &pm.environments {
                let _ = ctx.term.write_line(&format!("{e:?}"));
            }
        }
    }

    Ok(())
}
