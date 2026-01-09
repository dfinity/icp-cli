use anyhow::Context as _;
use candid::{IDLArgs, Principal, TypeEnv, types::Function};
use candid_parser::utils::CandidSource;
use clap::Args;
use dialoguer::console::Term;
use ic_agent::Agent;
use icp::context::Context;
use std::io::{self, Write};
use tracing::warn;

use crate::{commands::args, operations::misc::fetch_canister_metadata};

#[derive(Args, Debug)]
pub(crate) struct CallArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Name of canister method to call into
    pub(crate) method: String,

    /// String representation of canister call arguments
    pub(crate) args: String,
}

pub(crate) async fn exec(ctx: &Context, args: &CallArgs) -> Result<(), anyhow::Error> {
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
    let cargs =
        candid_parser::parse_idl_args(&args.args).context("failed to parse candid arguments")?;

    let arg_bytes = if let Some((env, method)) = get_candid_type(&agent, cid, &args.method).await {
        cargs
            .to_bytes_with_types(&env, &method.args)
            .context("failed to serialize candid arguments with specific types")?
    } else {
        warn!(
            "Could not fetch candid type for method '{}', serializing arguments with inferred types.",
            args.method
        );
        cargs
            .to_bytes()
            .context("failed to serialize candid arguments")?
    };

    let res = agent.update(&cid, &args.method).with_arg(arg_bytes).await?;

    let ret = IDLArgs::from_bytes(&res[..])?;

    print_candid_for_term(&mut Term::buffered_stdout(), &ret)
        .context("failed to print candid return value")?;

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

/// Gets the Candid type of a method on a canister by fetching its Candid interface.
///
/// This is a best effort function: it will succeed if
/// - the canister exposes its Candid interface in its metadata;
/// - the IDL file can be parsed and type checked in Rust parser;
/// - has an actor in the IDL file. If anything fails, it returns None.
async fn get_candid_type(
    agent: &Agent,
    canister_id: Principal,
    method_name: &str,
) -> Option<(TypeEnv, Function)> {
    let candid_interface = fetch_canister_metadata(&agent, canister_id, "candid:service").await?;
    let candid_source = CandidSource::Text(&candid_interface);
    let (env, ty) = candid_source.load().ok()?;
    let actor = ty?;
    let method = env.get_method(&actor, method_name).ok()?.clone();
    Some((env, method))
}
