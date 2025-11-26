use anyhow::Context as _;
use candid::IDLArgs;
use clap::Args;
use dialoguer::console::Term;
use icp::context::Context;
use std::io::{self, Write};

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

    let res = agent
        .update(&cid, &args.method)
        .with_arg(
            cargs
                .to_bytes()
                .context("failed to serialize candid arguments")?,
        )
        .await?;

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
