use crate::env::Env;
use clap::Parser;
use icp_identity::LoadIdentityError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct PrincipalCmd {}

pub fn exec(env: &Env, _cmd: PrincipalCmd) -> Result<(), PrincipalError> {
    let identity = env.load_identity()?;
    let principal = identity
        .sender()
        .map_err(|message| PrincipalError::IdentityError { message })?;
    println!("{principal}");
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum PrincipalError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityError },

    #[snafu(display("failed to load identity principal: {message}"))]
    IdentityError { message: String },
}
