use crate::env::Env;
use crate::options::IdentityOpt;
use clap::Parser;
use icp_identity::key::LoadIdentityInContextError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct PrincipalCmd {
    #[clap(flatten)]
    pub identity: IdentityOpt,
}

pub fn exec(env: &Env, cmd: PrincipalCmd) -> Result<(), PrincipalError> {
    env.require_identity(cmd.identity.name());

    let identity = env.identity()?;
    let principal = identity
        .sender()
        .map_err(|message| PrincipalError::IdentityError { message })?;
    println!("{principal}");
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum PrincipalError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("failed to load identity principal: {message}"))]
    IdentityError { message: String },
}
