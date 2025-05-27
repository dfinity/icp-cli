use clap::Parser;
use ic_agent::export::Principal;
use icp_identity::LoadIdentityError;
use parse_display::Display;
use serde::Serialize;
use snafu::Snafu;

use crate::env::Env;

#[derive(Parser)]
pub struct GetPrincipalCmd {}

pub fn exec(env: &Env, _cmd: GetPrincipalCmd) -> Result<GetPrincipalMessage, GetPrincipalError> {
    let identity = icp_identity::load_identity_in_context(env.dirs(), || todo!())?;
    let principal = identity
        .sender()
        .map_err(|message| GetPrincipalError::IdentityError { message })?;
    Ok(GetPrincipalMessage { principal })
}

#[derive(Debug, Snafu)]
pub enum GetPrincipalError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityError },
    #[snafu(display("failed to load identity principal: {message}"))]
    IdentityError { message: String },
}

#[derive(Serialize, Display)]
#[display("{principal}")]
pub struct GetPrincipalMessage {
    principal: Principal,
}
