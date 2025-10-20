use clap::Args;
use icp::identity::manifest::{
    ChangeDefaultsError, LoadIdentityManifestError, change_default_identity,
    load_identity_defaults, load_identity_list,
};

use crate::commands::{Context, Mode};

#[derive(Debug, Args)]
pub(crate) struct DefaultArgs {
    name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    ChangeDefault(#[from] ChangeDefaultsError),

    #[error(transparent)]
    LoadList(#[from] LoadIdentityManifestError),
}

pub(crate) async fn exec(ctx: &Context, args: &DefaultArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global | Mode::Project(_) => {
            // Load project directories
            let dir = ctx.dirs.identity();

            match &args.name {
                Some(name) => {
                    let list = load_identity_list(&dir)?;
                    change_default_identity(&dir, &list, name)?;
                    println!("Set default identity to {name}");
                }

                None => {
                    let defaults = load_identity_defaults(&dir)?;
                    println!("{}", defaults.default);
                }
            }
        }
    }

    Ok(())
}
