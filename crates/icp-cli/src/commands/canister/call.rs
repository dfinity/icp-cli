use std::io::{self, Write};

use candid::IDLArgs;
use clap::Args;
use dialoguer::console::Term;

use icp::context::{Context, GetAgentError, GetCanisterIdError};

use crate::commands::args;

#[derive(Args, Debug)]
pub(crate) struct CallArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Name of canister method to call into
    pub(crate) method: String,

    /// String representation of canister call arguments
    pub(crate) args: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("failed to parse candid arguments")]
    DecodeArgsError(#[from] candid_parser::Error),

    #[error("failed to serialize candid arguments")]
    Candid(#[from] candid::Error),

    #[error("failed to print candid return value")]
    WriteTermError(#[from] std::io::Error),

    #[error(transparent)]
    Call(#[from] ic_agent::AgentError),

    #[error(transparent)]
    GetAgent(#[from] GetAgentError),

    #[error(transparent)]
    GetCanisterId(#[from] GetCanisterIdError),
}

pub(crate) async fn exec(ctx: &Context, args: &CallArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Parse candid arguments
    let cargs = candid_parser::parse_idl_args(&args.args)?;

    let res = agent
        .update(&cid, &args.method)
        .with_arg(cargs.to_bytes()?)
        .await?;

    let ret = IDLArgs::from_bytes(&res[..])?;

    print_candid_for_term(&mut Term::buffered_stdout(), &ret)?;

    Ok(())
}

/// Pretty-prints IDLArgs detecting the terminal's width to avoid the 80-column default.
pub(crate) fn print_candid_for_term(term: &mut Term, args: &IDLArgs) -> io::Result<()> {
    if term.is_term() {
        let width = term.size().1 as usize;
        let pp_args = candid_parser::pretty::candid::value::pp_args(args);
        match pp_args.render(width, term) {
            Ok(()) => {
                writeln!(term)?;
            }
            Err(_) => {
                writeln!(term, "{args}")?;
            }
        }
    } else {
        writeln!(term, "{args}")?;
    }
    term.flush()?;
    Ok(())
}
