use crate::env::Env;
use clap::Parser;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct DefaultCmd {
    name: Option<String>,
}

pub fn exec(env: &Env, cmd: DefaultCmd) -> Result<(), DefaultIdentityError> {
    if let Some(name) = cmd.name {
        let list = icp_identity::manifest::load_identity_list(env.dirs())?;
        icp_identity::manifest::change_default_identity(env.dirs(), &list, &name)?;
        println!("Set default identity to {name}");
    } else {
        let defaults = icp_identity::manifest::load_identity_defaults(env.dirs())?;
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
        source: icp_identity::LoadIdentityError,
    },
}
