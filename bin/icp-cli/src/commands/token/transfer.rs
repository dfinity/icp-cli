use bigdecimal::{BigDecimal, num_bigint::ToBigInt};
use candid::{Decode, Encode, Nat, Principal};
use clap::Parser;
use ic_agent::AgentError;
use icp_identity::key::LoadIdentityInContextError;
use icrc_ledger_types::icrc1::{account::Account, transfer::TransferArg, transfer::TransferError};
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    #[clap(flatten)]
    pub environment: EnvironmentOpt,

    #[clap(flatten)]
    pub identity: IdentityOpt,

    #[clap(value_name = "RECEIVER")]
    pub receiver: Principal,

    #[clap(value_name = "AMOUNT")]
    pub amount: BigDecimal,
    // todo: subaccount, owner, account_id
}

pub async fn exec(
    ctx: &Context,
    parent_cmd: super::TokenArgs,
    cmd: Cmd,
) -> Result<(), CommandError> {
    // Load identity
    ctx.require_identity(cmd.identity.name());

    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    let token_address = parent_cmd
        .token_address()
        .ok_or(CommandError::InvalidToken {
            token: parent_cmd.token().to_string(),
        })?;

    let fee_future = agent
        .query(&token_address, "icrc1_fee")
        .with_arg(Encode!(&()).unwrap())
        .call();
    let decimals_future = agent
        .query(&token_address, "icrc1_decimals")
        .with_arg(Encode!(&()).unwrap())
        .call();
    let symbol_future = agent
        .query(&token_address, "icrc1_symbol")
        .with_arg(Encode!(&()).unwrap())
        .call();
    let (fee_response, decimals_response, symbol_response) =
        tokio::join!(fee_future, decimals_future, symbol_future);
    let fee_bytes = fee_response
        .map_err(|e| CommandError::TokenCanisterError { source: e })
        .unwrap();
    let decimals_bytes = decimals_response
        .map_err(|e| CommandError::TokenCanisterError { source: e })
        .unwrap();
    let symbol_bytes = symbol_response
        .map_err(|e| CommandError::TokenCanisterError { source: e })
        .unwrap();
    let fee = Decode!(&fee_bytes, Nat).expect("todo");
    let decimals = Decode!(&decimals_bytes, u8).expect("todo");
    let symbol = Decode!(&symbol_bytes, String).expect("todo");

    let token_units = Nat::from(
        (cmd.amount.clone() * 10u128.pow(decimals as u32))
            .to_bigint()
            .expect("todo")
            .to_biguint()
            .expect("todo"),
    );

    let transfer_response = agent
        .update(&token_address, "icrc1_transfer")
        .with_arg(
            Encode!(&TransferArg {
                from_subaccount: None,
                to: Account {
                    owner: cmd.receiver,
                    subaccount: None
                },
                fee: None,
                created_at_time: None,
                memo: None,
                amount: token_units,
            })
            .unwrap(),
        )
        .call_and_wait()
        .await
        .map_err(|e| CommandError::TokenCanisterError { source: e })?;
    let transfer_result = Decode!(&transfer_response, Result<Nat, TransferError>)
        .expect("Token does not follow the icrc1 standard");

    match transfer_result {
        Ok(block_index) => {
            println!(
                "Transferred {amount} {symbol} to {receiver} in block {block_index}.",
                amount = cmd.amount,
                symbol = symbol,
                receiver = cmd.receiver,
                block_index = block_index
            );
        }
        Err(error) => match error {
            TransferError::InsufficientFunds { balance } => {
                let balance = BigDecimal::from_biguint(balance.0, decimals as i64);
                let fee = BigDecimal::from_biguint(fee.0, decimals as i64);
                let required = cmd.amount + fee;
                println!(
                    "Insufficient funds. Balance: {balance} {symbol}, Required: {required} {symbol} (including fee)"
                );
            }
            TransferError::BadFee { .. } => {
                unreachable!("We do not specify a fee, so BadFee is not possible")
            }
            TransferError::BadBurn { min_burn_amount } => {
                println!("Cannot burn less than {min_burn_amount}.")
            }
            TransferError::TooOld => {
                unreachable!("We do not specify a created_at_time, so TooOld is not possible")
            }
            TransferError::CreatedInFuture { .. } => unreachable!(
                "We do not specify a created_at_time, so CreatedInFuture is not possible"
            ),
            TransferError::TemporarilyUnavailable => todo!(),
            TransferError::Duplicate { .. } => {
                unreachable!("We do not specify a created_at_time, so Duplicate is not possible")
            }
            TransferError::GenericError {
                error_code,
                message,
            } => {
                println!("Token canister returned generic error: {error_code}: {message}");
            }
        },
    };
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("Token name unknown: {token}"))]
    InvalidToken { token: String },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("Failed to talk to token canister: {source}"))]
    TokenCanisterError { source: AgentError },
}
