use std::io::{self, Write};

use candid::IDLArgs;
use clap::Args;
use dialoguer::console::Term;

use icp::context::{CanisterSelection, Context, EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;

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
    GetCanisterIdAndAgent(#[from] icp::context::GetCanisterIdAndAgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &CallArgs) -> Result<(), CommandError> {
    let canister_selection: CanisterSelection = args.cmd_args.canister.clone().into();
    let environment_selection: EnvironmentSelection =
        args.cmd_args.environment.clone().unwrap_or_default().into();
    let network_selection: NetworkSelection = match args.cmd_args.network.clone() {
        Some(network) => network.into_selection(),
        None => NetworkSelection::FromEnvironment,
    };
    let identity_selection: IdentitySelection = args.cmd_args.identity.clone().into();

    let (cid, agent) = ctx
        .get_canister_id_and_agent(
            &canister_selection,
            &environment_selection,
            &network_selection,
            &identity_selection,
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
