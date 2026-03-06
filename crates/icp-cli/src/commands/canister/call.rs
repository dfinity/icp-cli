use anyhow::{Context as _, anyhow, bail};
use candid::{Encode, IDLArgs, Nat, Principal, TypeEnv, types::Function};
use candid_parser::assist;
use candid_parser::parse_idl_args;
use clap::{Args, ValueEnum};
use dialoguer::console::Term;
use icp::context::Context;
use icp::fs;
use icp::manifest::InitArgsFormat;
use icp::parsers::CyclesAmount;
use icp::prelude::*;
use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};
use std::io::{self, Write};
use tracing::warn;

use crate::commands::args;
use crate::commands::candid::build::{get_candid_type, load_candid_file, pick_method};

/// How to interpret and display the call response blob.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum CallOutputMode {
    /// Try Candid, then UTF-8, then fall back to hex.
    #[default]
    Auto,
    /// Parse as Candid and pretty-print; error if parsing fails.
    Candid,
    /// Parse as UTF-8 text; error if invalid.
    Text,
    /// Print raw response as hex.
    Hex,
}

/// Make a canister call
#[derive(Args, Debug)]
pub(crate) struct CallArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Name of canister method to call into.
    /// If not provided, an interactive prompt will be launched.
    pub(crate) method: Option<String>,

    /// Call arguments, interpreted per `--args-format` (Candid by default).
    /// If not provided, an interactive prompt will be launched.
    #[arg(conflicts_with = "args_file")]
    pub(crate) args: Option<String>,

    /// Path to a file containing call arguments.
    #[arg(long, conflicts_with = "args")]
    pub(crate) args_file: Option<PathBuf>,

    /// Format of the call arguments.
    #[arg(long, default_value = "candid")]
    pub(crate) args_format: InitArgsFormat,

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
    #[arg(long, requires = "proxy", default_value = "0")]
    pub(crate) cycles: CyclesAmount,

    /// Sends a query request to a canister instead of an update request.
    ///
    /// Query calls are faster but return uncertified responses.
    /// Cannot be used with --proxy (proxy calls are always update calls).
    #[arg(long, conflicts_with = "proxy")]
    pub(crate) query: bool,

    /// How to interpret and display the response.
    #[arg(long, short, default_value = "auto")]
    pub(crate) output: CallOutputMode,

    /// Optionally provide a local Candid file describing the canister interface,
    /// instead of looking it up from canister metadata.
    #[arg(short = 'c', long)]
    pub(crate) candid_file: Option<PathBuf>,
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

    let candid_types = if let Some(path) = &args.candid_file {
        Some(load_candid_file(path)?)
    } else {
        get_candid_type(&agent, cid).await
    };

    let method = if let Some(method) = &args.method {
        method.clone()
    } else if let Some(interface) = &candid_types {
        pick_method(interface, "Select a method to call")?
    } else {
        bail!(
            "method name was not provided and could not fetch candid type to assist method selection"
        );
    };
    let declared_method =
        candid_types.and_then(|i| Some((i.env.clone(), i.get_method(&method)?.clone())));
    enum ResolvedArgs {
        Candid(IDLArgs),
        Bytes(Vec<u8>),
    }

    let resolved_args = match (&args.args, &args.args_file) {
        (Some(value), None) => {
            if args.args_format == InitArgsFormat::Bin {
                bail!("--args-format bin requires --args-file, not a positional argument");
            }
            Some(match args.args_format {
                InitArgsFormat::Candid => ResolvedArgs::Candid(
                    parse_idl_args(value).context("failed to parse Candid arguments")?,
                ),
                InitArgsFormat::Hex => ResolvedArgs::Bytes(
                    hex::decode(value).context("failed to decode hex arguments")?,
                ),
                InitArgsFormat::Bin => unreachable!(),
            })
        }
        (None, Some(file_path)) => Some(match args.args_format {
            InitArgsFormat::Bin => {
                let bytes = fs::read(file_path).context("failed to read args file")?;
                ResolvedArgs::Bytes(bytes)
            }
            InitArgsFormat::Hex => {
                let content = fs::read_to_string(file_path).context("failed to read args file")?;
                ResolvedArgs::Bytes(
                    hex::decode(content.trim()).context("failed to decode hex from file")?,
                )
            }
            InitArgsFormat::Candid => {
                let content = fs::read_to_string(file_path).context("failed to read args file")?;
                ResolvedArgs::Candid(
                    parse_idl_args(content.trim()).context("failed to parse Candid from file")?,
                )
            }
        }),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
    };

    let arg_bytes = match (&declared_method, resolved_args) {
        (_, None) if args.args_format != InitArgsFormat::Candid => {
            bail!("arguments must be provided when --args-format is not candid");
        }
        (None, None) => bail!(
            "arguments were not provided and could not fetch candid type to assist building arguments"
        ),
        (None, Some(ResolvedArgs::Bytes(bytes))) => bytes,
        (None, Some(ResolvedArgs::Candid(arguments))) => {
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
        (Some(_), Some(ResolvedArgs::Bytes(bytes))) => bytes,
        (Some((type_env, func)), Some(ResolvedArgs::Candid(arguments))) => arguments
            .to_bytes_with_types(type_env, &func.args)
            .context("failed to serialize candid arguments with specific types")?,
    };

    let res = if let Some(proxy_cid) = args.proxy {
        // Route the call through the proxy canister
        let proxy_args = ProxyArgs {
            canister_id: cid,
            method: method.clone(),
            args: arg_bytes,
            cycles: Nat::from(args.cycles.get()),
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
        if let Some((_, func)) = &declared_method
            && !func.is_query()
        {
            bail!(
                "`{method}` is an update method, not a query method. \
                 Run the command without `--query`.",
            );
        }
        agent
            .query(&cid, &method)
            .with_arg(arg_bytes)
            .call()
            .await?
    } else {
        // Direct update call to the target canister
        agent.update(&cid, &method).with_arg(arg_bytes).await?
    };

    let mut term = Term::buffered_stdout();
    let res_hex = || format!("response (hex): {}", hex::encode(&res));

    match args.output {
        CallOutputMode::Auto => {
            if let Ok(ret) = try_decode_candid(&res, declared_method.as_ref()) {
                print_candid_for_term(&mut term, &ret)
                    .context("failed to print candid return value")?;
            } else if let Ok(s) = std::str::from_utf8(&res) {
                writeln!(term, "{s}")?;
                term.flush()?;
            } else {
                writeln!(term, "{}", hex::encode(&res))?;
                term.flush()?;
            }
        }
        CallOutputMode::Candid => {
            let ret = try_decode_candid(&res, declared_method.as_ref()).with_context(res_hex)?;
            print_candid_for_term(&mut term, &ret)
                .context("failed to print candid return value")?;
        }
        CallOutputMode::Text => {
            let s = std::str::from_utf8(&res)
                .with_context(res_hex)
                .context("response is not valid UTF-8")?;
            writeln!(term, "{s}")?;
            term.flush()?;
        }
        CallOutputMode::Hex => {
            writeln!(term, "{}", hex::encode(&res))?;
            term.flush()?;
        }
    }

    Ok(())
}

/// Tries to decode the response as Candid. Returns `None` if decoding fails.
fn try_decode_candid(
    res: &[u8],
    candid_types: Option<&(TypeEnv, Function)>,
) -> Result<IDLArgs, anyhow::Error> {
    match candid_types {
        Some((type_env, func)) => IDLArgs::from_bytes_with_types(res, type_env, &func.rets)
            .map_err(|e| anyhow!("failed to parse Candid: {e}")),
        None => IDLArgs::from_bytes(res).map_err(|e| anyhow!("failed to parse Candid: {e}")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use candid_parser::utils::CandidSource;

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
