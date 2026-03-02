use candid::Principal;
use clap::{Args, ValueEnum};
use ic_ledger_types::{AccountIdentifier, Subaccount};
use icp::context::Context;
use icrc_ledger_types::icrc1::account::Account;

use crate::commands::parsers::parse_subaccount;
use crate::options::IdentityOpt;

/// The account identifier format to display
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum OutputFormat {
    /// ICP ledger account identifier
    #[default]
    Ledger,
    /// ICRC-1 account identifier
    Icrc1,
}

/// Display the ICP ledger or ICRC-1 account identifier for the current identity
#[derive(Debug, Args)]
pub(crate) struct AccountIdArgs {
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    /// Convert this Principal instead of the current identity's Principal
    #[arg(long = "of-principal", conflicts_with = "identity")]
    pub(crate) of_principal: Option<Principal>,

    /// Specify a subaccount
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) of_subaccount: Option<[u8; 32]>,

    /// Account identifier format to display [default: ledger]
    #[arg(long, default_value = "ledger")]
    pub(crate) format: OutputFormat,
}

pub(crate) async fn exec(ctx: &Context, args: &AccountIdArgs) -> Result<(), anyhow::Error> {
    let principal = if let Some(p) = &args.of_principal {
        *p
    } else {
        let id = ctx.get_identity(&args.identity.clone().into()).await?;
        id.sender()
            .map_err(|e| anyhow::anyhow!("failed to load identity principal: {e}"))?
    };

    match args.format {
        OutputFormat::Ledger => {
            let account_id = AccountIdentifier::new(
                &principal,
                &args
                    .of_subaccount
                    .map(Subaccount)
                    .unwrap_or(Subaccount([0; 32])),
            );
            println!("{account_id}");
        }
        OutputFormat::Icrc1 => {
            if let Some(subaccount) = args.of_subaccount {
                let account = Account {
                    owner: principal,
                    subaccount: Some(subaccount),
                };
                println!("{account}");
            } else {
                println!("{principal}");
            }
        }
    }
    Ok(())
}
