use candid::Principal;
use clap::Args;
use ic_ledger_types::{AccountIdentifier, Subaccount};
use icp::context::Context;
use icrc_ledger_types::icrc1::account::Account;

use crate::commands::parsers::parse_subaccount;
use crate::options::IdentityOpt;

/// Display the ICP ledger and ICRC-1 account identifiers for the current identity
#[derive(Debug, Args)]
pub(crate) struct AccountIdArgs {
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    /// Convert this Principal instead of the current identity's Principal
    #[arg(long = "of-principal", conflicts_with = "identity")]
    pub(crate) of_principal: Option<Principal>,

    /// Specify a subaccount. If absent, the ICRC-1 account will be omitted as it is just the principal
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) of_subaccount: Option<[u8; 32]>,
}

pub(crate) async fn exec(ctx: &Context, args: &AccountIdArgs) -> Result<(), anyhow::Error> {
    let principal = if let Some(p) = &args.of_principal {
        *p
    } else {
        let id = ctx.get_identity(&args.identity.clone().into()).await?;
        id.sender()
            .map_err(|e| anyhow::anyhow!("failed to load identity principal: {e}"))?
    };

    let account_id = AccountIdentifier::new(
        &principal,
        &args
            .of_subaccount
            .map(Subaccount)
            .unwrap_or(Subaccount([0; 32])),
    );

    println!("ICP ledger: {account_id}");
    if args.of_subaccount.is_some() {
        let account = Account {
            owner: principal,
            subaccount: args.of_subaccount,
        };
        println!("ICRC-1: {account}");
    }
    Ok(())
}
