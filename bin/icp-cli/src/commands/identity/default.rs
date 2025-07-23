use crate::context::Context;
use clap::Parser;
use icp_identity::manifest::{change_default_identity, load_identity_defaults, load_identity_list};
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct DefaultCmd {
    name: Option<String>,
}

pub fn exec(ctx: &Context, cmd: DefaultCmd) -> Result<(), DefaultIdentityError> {
    let dirs = ctx.dirs();
    if let Some(name) = cmd.name {
        let list = load_identity_list(dirs)?;
        change_default_identity(dirs, &list, &name)?;
        println!("Set default identity to {name}");
    } else {
        let defaults = load_identity_defaults(dirs)?;
        println!("{}", defaults.default);
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DefaultIdentityError {
    #[snafu(transparent)]
    ChangeDefault {
        source: icp_identity::manifest::ChangeDefaultsError,
    },

    #[snafu(transparent)]
    LoadList {
        source: icp_identity::manifest::LoadIdentityManifestError,
    },
}
