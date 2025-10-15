use clap::Parser;
use icp::identity::manifest::{
    ChangeDefaultsError, LoadIdentityManifestError, change_default_identity,
    load_identity_defaults, load_identity_list,
};
use snafu::Snafu;

use crate::commands::Context;

#[derive(Debug, Parser)]
pub struct DefaultCmd {
    name: Option<String>,
}

#[derive(Debug, Snafu)]
pub enum DefaultIdentityError {
    #[snafu(transparent)]
    ChangeDefault { source: ChangeDefaultsError },

    #[snafu(transparent)]
    LoadList { source: LoadIdentityManifestError },
}

pub fn exec(ctx: &Context, cmd: DefaultCmd) -> Result<(), DefaultIdentityError> {
    // Load project directories
    let dir = ctx.dirs.identity();

    match cmd.name {
        Some(name) => {
            let list = load_identity_list(&dir)?;
            change_default_identity(&dir, &list, &name)?;
            tracing::info!("Set default identity to {name}");
        }

        None => {
            let defaults = load_identity_defaults(&dir)?;
            tracing::info!("{}", defaults.default);
        }
    }

    Ok(())
}
