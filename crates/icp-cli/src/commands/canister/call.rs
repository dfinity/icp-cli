use anyhow::{Context as _, bail};
use candid::{IDLArgs, Principal, TypeEnv, types::Function};
use candid_parser::assist;
use candid_parser::utils::CandidSource;
use clap::Args;
use dialoguer::console::Term;
use ic_agent::Agent;
use icp::context::Context;
use icp::prelude::*;
use std::io::{self, Write};
use tracing::warn;

use crate::{
    commands::args,
    operations::misc::{ParsedArguments, fetch_canister_metadata, parse_args},
};

#[derive(Args, Debug)]
pub(crate) struct CallArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Name of canister method to call into
    pub(crate) method: String,

    /// Canister call arguments.
    /// Can be:
    ///
    /// - Hex-encoded bytes (e.g., `4449444c00`)
    ///
    /// - Candid text format (e.g., `(42)` or `(record { name = "Alice" })`)
    ///
    /// - File path (e.g., `args.txt` or `./path/to/args.candid`)
    ///   The file should contain either hex or Candid format arguments.
    ///
    /// If not provided, an interactive prompt will be launched to help build the arguments.
    pub(crate) args: Option<String>,
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

    let candid_types = get_candid_type(&agent, cid, &args.method).await;

    let parsed_args = args
        .args
        .as_ref()
        .map(|s| {
            let cwd =
                dunce::canonicalize(".").context("Failed to get current working directory")?;
            let cwd =
                PathBuf::try_from(cwd).context("Current directory path is not valid UTF-8")?;
            parse_args(s, &cwd)
        })
        .transpose()?;

    let arg_bytes = match (candid_types, parsed_args) {
        (None, None) => bail!(
            "arguments was not provided and could not fetch candid type to assist building arguments"
        ),
        (None, Some(ParsedArguments::Hex(bytes))) => bytes,
        (None, Some(ParsedArguments::Candid(arguments))) => {
            warn!("could not fetch candid type, serializing arguments with inferred types.");
            arguments
                .to_bytes()
                .context("failed to serialize candid arguments")?
        }
        (Some((type_env, func)), None) => {
            // interactive argument building using candid assist
            let context = assist::Context::new(type_env);
            eprintln!("Please use the following interactive prompt to build the arguments.");
            let arguments = assist::input_args(&context, &func.args)?;
            eprintln!("Sending the following argument:\n{arguments}\n");
            eprintln!("Do you want to send this message? [y/N]");
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !["y", "Y", "yes", "Yes", "YES"].contains(&input.trim()) {
                eprintln!("User cancelled.");
                return Ok(());
            }
            arguments
                .to_bytes()
                .context("failed to serialize candid arguments")?
        }
        (Some(_), Some(ParsedArguments::Hex(bytes))) => {
            // Hex bytes are already encoded, use as-is
            bytes
        }
        (Some((type_env, func)), Some(ParsedArguments::Candid(arguments))) => arguments
            .to_bytes_with_types(&type_env, &func.args)
            .context("failed to serialize candid arguments with specific types")?,
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
    let candid_interface = fetch_canister_metadata(agent, canister_id, "candid:service").await?;
    let candid_source = CandidSource::Text(&candid_interface);
    let (type_env, ty) = candid_source.load().ok()?;
    let actor = ty?;
    let func = type_env.get_method(&actor, method_name).ok()?.clone();
    Some((type_env, func))
}
