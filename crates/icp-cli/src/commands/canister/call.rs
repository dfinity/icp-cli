use anyhow::{Context as _, bail};
use candid::{Encode, IDLArgs, Nat, Principal, TypeEnv, types::Function};
use candid_parser::assist;
use candid_parser::utils::CandidSource;
use clap::Args;
use dialoguer::console::Term;
use ic_agent::Agent;
use icp::context::Context;
use icp::prelude::*;
use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};
use std::io::{self, Write};
use tracing::warn;

use crate::{
    commands::args,
    operations::misc::{ParsedArguments, fetch_canister_metadata, parse_args},
};

/// Make a canister call
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

    /// Principal of a proxy canister to route the call through.
    ///
    /// When specified, instead of calling the target canister directly,
    /// the call will be sent to the proxy canister's `proxy` method,
    /// which forwards it to the target canister.
    #[arg(long)]
    pub(crate) proxy: Option<Principal>,

    /// Cycles to forward with the proxied call.
    ///
    /// Only used when --proxy is specified. Defaults to 0.
    #[arg(long, requires = "proxy", value_parser = icp::parsers::parse_cycles_amount, default_value = "0")]
    pub(crate) cycles: u128,

    /// Sends a query request to a canister instead of an update request.
    ///
    /// Query calls are faster but return uncertified responses.
    /// Cannot be used with --proxy (proxy calls are always update calls).
    #[arg(long, conflicts_with = "proxy")]
    pub(crate) query: bool,
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

    let arg_bytes = match (&candid_types, parsed_args) {
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
            let context = assist::Context::new(type_env.clone());
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
            .to_bytes_with_types(type_env, &func.args)
            .context("failed to serialize candid arguments with specific types")?,
    };

    let res = if let Some(proxy_cid) = args.proxy {
        // Route the call through the proxy canister
        let proxy_args = ProxyArgs {
            canister_id: cid,
            method: args.method.clone(),
            args: arg_bytes,
            cycles: Nat::from(args.cycles),
        };
        let proxy_arg_bytes =
            Encode!(&proxy_args).context("failed to encode proxy call arguments")?;

        let proxy_res = agent
            .update(&proxy_cid, "proxy")
            .with_arg(proxy_arg_bytes)
            .await?;

        let proxy_result: (ProxyResult,) =
            candid::decode_args(&proxy_res).context("failed to decode proxy canister response")?;

        match proxy_result.0 {
            ProxyResult::Ok(ok) => ok.result,
            ProxyResult::Err(err) => bail!(err.format_error()),
        }
    } else if args.query {
        // Preemptive check: error if Candid shows this is an update method
        if let Some((_, func)) = &candid_types
            && !func.is_query()
        {
            bail!(
                "`{}` is an update method, not a query method. \
                 Run the command without `--query`.",
                args.method
            );
        }
        agent
            .query(&cid, &args.method)
            .with_arg(arg_bytes)
            .call()
            .await?
    } else {
        // Direct update call to the target canister
        agent.update(&cid, &args.method).with_arg(arg_bytes).await?
    };

    let ret = match &candid_types {
        Some((type_env, func)) => IDLArgs::from_bytes_with_types(&res[..], type_env, &func.rets)
            .context("failed to decode candid response with types")?,
        None => IDLArgs::from_bytes(&res[..]).context("failed to decode candid response")?,
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_decoding_preserves_record_field_names() {
        // Encode a record — field names become hashes in the Candid binary format
        let args = candid_parser::parse_idl_args(
            r#"(record { network = "regtest"; bitcoin_canister_id = "abc" })"#,
        )
        .unwrap();
        let bytes = args.to_bytes().unwrap();

        // Without types: field names are lost, displayed as hash numbers
        let untyped = IDLArgs::from_bytes(&bytes).unwrap();
        let untyped_str = format!("{untyped}");
        assert!(
            !untyped_str.contains("network"),
            "untyped decoding should not contain field names: {untyped_str}"
        );

        // With types: field names are restored from the type environment
        let did = r#"
            type config = record { network : text; bitcoin_canister_id : text };
            service : { "get_config" : () -> (config) query }
        "#;
        let source = CandidSource::Text(did);
        let (type_env, ty) = source.load().unwrap();
        let actor = ty.unwrap();
        let func = type_env.get_method(&actor, "get_config").unwrap().clone();

        let typed = IDLArgs::from_bytes_with_types(&bytes, &type_env, &func.rets).unwrap();
        let typed_str = format!("{typed}");
        assert!(
            typed_str.contains("network"),
            "typed decoding should contain 'network': {typed_str}"
        );
        assert!(
            typed_str.contains("bitcoin_canister_id"),
            "typed decoding should contain 'bitcoin_canister_id': {typed_str}"
        );
    }

    #[test]
    fn is_query_detects_method_types() {
        let did = r#"
            service : {
                "get_value" : () -> (text) query;
                "set_value" : (text) -> ()
            }
        "#;
        let source = CandidSource::Text(did);
        let (type_env, ty) = source.load().unwrap();
        let actor = ty.unwrap();

        let query_func = type_env.get_method(&actor, "get_value").unwrap();
        assert!(
            query_func.is_query(),
            "get_value should be detected as query"
        );

        let update_func = type_env.get_method(&actor, "set_value").unwrap();
        assert!(
            !update_func.is_query(),
            "set_value should be detected as update"
        );
    }
}
