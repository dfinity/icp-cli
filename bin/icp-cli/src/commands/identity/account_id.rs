use crate::context::Context;
use crate::options::IdentityOpt;
use clap::Parser;
use ic_ledger_types::{AccountIdentifier, Subaccount};
use icp_identity::key::LoadIdentityInContextError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct AccountIdCmd {
    #[clap(flatten)]
    pub identity: IdentityOpt,
}

pub fn exec(ctx: &Context, cmd: AccountIdCmd) -> Result<(), AccountIdError> {
    ctx.require_identity(cmd.identity.name());

    let identity = ctx.identity()?;
    let principal = identity
        .sender()
        .map_err(|message| AccountIdError::IdentityError { message })?;
    let account = AccountIdentifier::new(&principal, &Subaccount([0; 32]));
    println!("{account}");
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum AccountIdError {
    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("failed to load identity principal: {message}"))]
    IdentityError { message: String },
}
