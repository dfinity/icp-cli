use bigdecimal::{BigDecimal, num_bigint::ToBigInt};
use candid::{Decode, Encode, Nat, Principal};
use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError},
};

use crate::{
    commands::{Context, Mode, args, token::TOKEN_LEDGER_CIDS},
    options::IdentityOpt,
};

#[derive(Debug, Args)]
pub(crate) struct TransferArgs {
    /// Token amount to transfer
    pub(crate) amount: BigDecimal,

    /// The receiver of the token transfer
    pub(crate) receiver: Principal,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[arg(long)]
    pub(crate) network: Option<args::Network>,

    #[arg(long)]
    pub(crate) environment: Option<args::Environment>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("an invalid argument was provided")]
    Args,

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("failed to get identity principal: {err}")]
    Principal { err: String },

    #[error(transparent)]
    Update(#[from] AgentError),

    #[error(transparent)]
    Candid(#[from] candid::Error),

    #[error("invalid amount")]
    Amount,

    #[error("transfer failed: {err}")]
    Transfer { err: String },

    #[error("insufficient funds. balance: {balance} {symbol}, required: {required} {symbol}")]
    InsufficientFunds {
        symbol: String,
        balance: BigDecimal,
        required: BigDecimal,
    },
}

pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
    args: &TransferArgs,
) -> Result<(), CommandError> {
    let (agent,) = match &ctx.mode {
        Mode::Global => {
            // Argument (Network)
            let Some(args::Network::Url(url)) = args.network.clone() else {
                return Err(CommandError::Args);
            };

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Agent
            let agent = ctx.agent.create(id, &url).await?;

            (agent,)
        }

        Mode::Project(pdir) => {
            // Argument (Environment)
            let args::Environment::Name(env) = args.environment.clone().unwrap_or_default();

            // Load project
            let p = ctx.project.load(pdir).await?;

            // Load identity
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            // Load target environment
            let env = p
                .environments
                .get(&env)
                .ok_or(CommandError::EnvironmentNotFound { name: env })?;

            // Access network
            let access = ctx.network.access(&env.network).await?;

            // Agent
            let agent = ctx.agent.create(id, &access.url).await?;

            if let Some(k) = access.root_key {
                agent.set_root_key(k);
            }

            (agent,)
        }
    };

    // Obtain ledger address
    let cid = match TOKEN_LEDGER_CIDS.get(token) {
        // Given token matched known token names
        Some(cid) => cid.to_string(),

        // Given token is not known, indicating it's either already a canister id
        // or is simply a name of a token we do not know of
        None => token.to_string(),
    };

    // Parse the canister id
    let cid = Principal::from_text(cid).map_err(|err| CommandError::Principal {
        err: err.to_string(),
    })?;

    // Perform the required ledger calls
    let (fee, decimals, symbol) = tokio::join!(
        //
        // Obtain token transfer fee
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_fee")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await?;

            // Decode response
            Ok::<_, CommandError>(Decode!(&resp, Nat)?)
        },
        //
        // Obtain the number of decimals the token uses
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_decimals")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await?;

            // Decode response
            Ok::<_, CommandError>(Decode!(&resp, u8)?)
        },
        //
        // Obtain the symbol of the token
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_symbol")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await?;

            // Decode response
            Ok::<_, CommandError>(Decode!(&resp, String)?)
        },
    );

    // Check for errors
    let (Nat(fee), decimals, symbol) = (
        fee?,             //
        decimals? as u32, //
        symbol?,          //
    );

    // Calculate units of token to transfer
    // Ledgers do not work in decimals and instead use the smallest non-divisible unit of the token
    let ledger_amount = args.amount.clone() * 10u128.pow(decimals);

    // Convert amount to big decimal
    let ledger_amount = ledger_amount
        .to_bigint()
        .ok_or(CommandError::Amount)?
        .to_biguint()
        .ok_or(CommandError::Amount)?;

    let ledger_amount = Nat::from(ledger_amount);
    let display_amount = BigDecimal::from_biguint(ledger_amount.0.clone(), decimals as i64);

    // Prepare transfer
    let receiver = Account {
        owner: args.receiver,
        subaccount: None,
    };

    let arg = TransferArg {
        // Transfer amount
        amount: ledger_amount.clone(),

        // Transfer destination
        to: receiver,

        // Other
        from_subaccount: None,
        fee: None,
        created_at_time: None,
        memo: None,
    };

    // Perform transfer
    let resp = agent
        .update(&cid, "icrc1_transfer")
        .with_arg(Encode!(&arg)?)
        .call_and_wait()
        .await?;

    // Parse response
    let resp = Decode!(&resp, Result<Nat, TransferError>)?;

    // Process response
    let idx = resp.map_err(|err| match err {
        // Special case for insufficient funds
        TransferError::InsufficientFunds { balance } => {
            let balance = BigDecimal::from_biguint(
                balance.0,       // balance
                decimals as i64, // decimals
            );

            let fee = BigDecimal::from_biguint(
                fee,             // fee
                decimals as i64, // decimals
            );

            CommandError::InsufficientFunds {
                symbol: symbol.clone(),
                balance,
                required: args.amount.clone() + fee,
            }
        }

        _ => CommandError::Transfer {
            err: err.to_string(),
        },
    })?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Transferred {display_amount} {symbol} to {receiver} in block {idx}"
    ));

    Ok(())
}
