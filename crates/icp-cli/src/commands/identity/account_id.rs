use candid::Principal;
use clap::Args;
use ic_ledger_types::{AccountIdentifier, Subaccount};
use icp::context::Context;

use crate::options::IdentityOpt;

#[derive(Debug, Args)]
pub(crate) struct AccountIdArgs {
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    /// Convert this Principal instead of the current identity's Principal
    #[arg(long = "of-principal", conflicts_with = "identity")]
    pub(crate) of_principal: Option<Principal>,
}

pub(crate) async fn exec(ctx: &Context, args: &AccountIdArgs) -> Result<(), anyhow::Error> {
    let principal = if let Some(p) = &args.of_principal {
        *p
    } else {
        let id = ctx.get_identity(&args.identity.clone().into()).await?;
        id.sender()
            .map_err(|e| anyhow::anyhow!("failed to load identity principal: {e}"))?
    };

    let account_id = AccountIdentifier::new(&principal, &Subaccount([0; 32]));

    println!("{account_id}");

    Ok(())
}
