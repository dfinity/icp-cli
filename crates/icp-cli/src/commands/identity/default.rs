use clap::Args;
use icp::{
    fs::lock::LockError,
    identity::manifest::{
        ChangeDefaultsError, IdentityDefaults, IdentityList, LoadIdentityManifestError,
        change_default_identity,
    },
};

use crate::commands::Context;

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

    #[error(transparent)]
    LoadLockError(#[from] LockError),
}

pub(crate) async fn exec(ctx: &Context, args: &DefaultArgs) -> Result<(), CommandError> {
    // Load project directories
    let dirs = ctx.dirs.identity()?;

    match &args.name {
        Some(name) => {
            dirs.with_write(async |dirs| {
                let list = IdentityList::load_from(dirs.read())?;
                change_default_identity(dirs, &list, name)?;
                println!("Set default identity to {name}");
                Ok(())
            })
            .await?
        }

        None => {
            let defaults = dirs
                .with_read(async |dirs| IdentityDefaults::load_from(dirs))
                .await??;
            println!("{}", defaults.default);
            Ok(())
        }
    }
}
