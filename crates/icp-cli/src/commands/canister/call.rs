use std::io::{self, Write};

use candid::IDLArgs;
use clap::Args;
use dialoguer::console::Term;
use icp::{agent, identity, network};

use crate::{
    commands::{args::{self, ArgValidationError}, helpers::{get_agent_for_env, get_agent_for_network, get_canister_id_for_env}, Context},
    store_id::LookupError,
};

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
    Shared(#[from] ArgValidationError),
}

pub(crate) async fn exec(ctx: &Context, args: &CallArgs) -> Result<(), CommandError> {
    let arg_canister = args.cmd_args.canister.clone();
    let arg_environment = args.cmd_args.environment.clone();
    let arg_network = args.cmd_args.network.clone();
    let arg_identity = args.cmd_args.identity.clone();

    let (cid, agent) = match (arg_canister, &arg_environment, arg_network) {
        (_, args::Environment::Name(_), Some(_)) => {
            // Both an environment and a network are specified this is an error
            return Err(ArgValidationError::EnvironmentAndNetworkSpecified.into());
        },
        (args::Canister::Name(_), args::Environment::Default(_), Some(_)) => {
            // This is not allowed, we should not use name with an environment not a network
            return Err(ArgValidationError::AmbiguousCanisterName.into());
        },
        (args::Canister::Name(cname), _, None) => {
            // A canister name was specified so we must be in a project

            let agent = get_agent_for_env(ctx, &arg_identity, &arg_environment).await?;
            let cid = get_canister_id_for_env(ctx, &cname, &arg_environment).await?;

            (cid, agent)
        },
        (args::Canister::Principal(principal), _, None) => {
            // Call by canister_id to the environment specified

            let agent = get_agent_for_env(ctx, &arg_identity, &arg_environment).await?;

            (principal, agent)
        },
        (args::Canister::Principal(principal), args::Environment::Default(_), Some(network)) => {
            // Should handle known networks by name

            let agent = get_agent_for_network(ctx, &arg_identity, &network).await?;
            (principal, agent)
        },
    };


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
