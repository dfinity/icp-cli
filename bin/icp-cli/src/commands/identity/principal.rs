use crate::env::Env;
use clap::Parser;
use ic_agent::export::Principal;
use icp_identity::LoadIdentityError;
use parse_display::Display;
use serde::Serialize;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct PrincipalCmd {}

pub fn exec(env: &Env, _cmd: PrincipalCmd) -> Result<PrincipalMessage, PrincipalError> {
    let identity = env.load_identity()?;
    let principal = identity
        .sender()
        .map_err(|message| PrincipalError::IdentityError { message })?;
    Ok(PrincipalMessage { principal })
}

#[derive(Debug, Snafu)]
pub enum PrincipalError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityError },

    #[snafu(display("failed to load identity principal: {message}"))]
    IdentityError { message: String },
}

#[derive(Serialize, Display)]
#[display("{principal}")]
#[serde(rename_all = "kebab-case")]
pub struct PrincipalMessage {
    principal: Principal,
}
