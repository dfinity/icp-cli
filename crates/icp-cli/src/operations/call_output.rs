//! Rendering a canister call's response.
//!
//! Shared by `icp canister call` and `icp message send`: the two resolve an
//! interface from different places — a project and the network, versus the text
//! embedded in a signed message — but a reply must print the same either way.

use anyhow::{Context as _, anyhow};
use candid::types::{Type, TypeInner};
use candid::{IDLArgs, Principal, TypeEnv, types::Function};
use candid_parser::utils::CandidSource;
use clap::ValueEnum;
use dialoguer::console::Term;
use ic_agent::Agent;
use icp::prelude::*;
use serde::Serialize;
use std::io::{self, Write};
use tracing::error;

use crate::operations::misc::fetch_canister_metadata;

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

/// Writes a call's response to stdout, in whichever of the two shapes was asked
/// for.
///
/// Lifted out of `canister call` unchanged so that `message send` renders a
/// reply identically: same `--output` modes, same `--json` envelope, same single
/// flush of the buffered terminal covering both paths.
pub(crate) fn print_response(
    res: &[u8],
    mode: CallOutputMode,
    method: Option<&(TypeEnv, Function)>,
    json: bool,
) -> Result<(), anyhow::Error> {
    let mut term = Term::buffered_stdout();
    let decoded = decode_response(res, mode, method);

    if json {
        let envelope = JsonCallResponse::build(res, decoded.as_ref().ok());
        let write_result = serde_json::to_writer(&term, &envelope);
        match (write_result, decoded) {
            (Ok(()), decode_result) => {
                decode_result?;
            }
            (Err(write_err), Err(decode_err)) => {
                // Prefer the decode error; the write failure is incidental.
                error!("failed to write JSON response: {write_err}");
                return Err(decode_err);
            }
            (Err(write_err), Ok(_)) => {
                return Err(write_err).context("failed to write JSON response");
            }
        }
    } else {
        match decoded? {
            Decoded::Candid(ret) => print_candid_for_term(&mut term, &ret)
                .context("failed to print candid return value")?,
            Decoded::Text(s) => writeln!(term, "{s}")?,
            Decoded::Bytes => writeln!(term, "{}", hex::encode(res))?,
        }
    }

    // term is buffered; this single flush covers all output paths (json and non-json).
    term.flush()?;
    Ok(())
}

/// A response decoded according to the requested `CallOutputMode`.
pub(crate) enum Decoded {
    Candid(IDLArgs),
    Text(String),
    /// No decoding was attempted or all attempts failed; emit raw bytes as hex.
    Bytes,
}

pub(crate) fn decode_response(
    res: &[u8],
    mode: CallOutputMode,
    method: Option<&(TypeEnv, Function)>,
) -> Result<Decoded, anyhow::Error> {
    let res_hex = || format!("response (hex): {}", hex::encode(res));
    match mode {
        CallOutputMode::Auto => {
            if let Ok(args) = try_decode_candid(res, method) {
                Ok(Decoded::Candid(args))
            } else if let Ok(s) = std::str::from_utf8(res) {
                Ok(Decoded::Text(s.to_string()))
            } else {
                Ok(Decoded::Bytes)
            }
        }
        CallOutputMode::Candid => try_decode_candid(res, method)
            .map(Decoded::Candid)
            .with_context(res_hex),
        CallOutputMode::Text => std::str::from_utf8(res)
            .map(|s| Decoded::Text(s.to_string()))
            .with_context(res_hex)
            .context("response is not valid UTF-8"),
        CallOutputMode::Hex => Ok(Decoded::Bytes),
    }
}

#[derive(Serialize)]
struct JsonCallResponse {
    response_bytes: String,
    response_text: Option<String>,
    response_candid: Option<String>,
}

impl JsonCallResponse {
    fn build(res: &[u8], decoded: Option<&Decoded>) -> Self {
        Self {
            response_bytes: hex::encode(res),
            response_text: match decoded {
                Some(Decoded::Text(s)) => Some(s.clone()),
                _ => None,
            },
            response_candid: match decoded {
                Some(Decoded::Candid(args)) => Some(format!("{args}")),
                _ => None,
            },
        }
    }
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
    Ok(())
}

/// Gets the Candid type of a method on a canister by fetching its Candid interface.
///
/// This is a best effort function: it will succeed if
/// - the canister exposes its Candid interface in its metadata;
/// - the IDL file can be parsed and type checked in Rust parser;
/// - has an actor in the IDL file. If anything fails, it returns None.
pub(crate) async fn get_candid_type(
    agent: &Agent,
    canister_id: Principal,
) -> Option<CanisterInterface> {
    let candid_interface = fetch_canister_metadata(agent, canister_id, "candid:service").await?;
    CanisterInterface::from_text(candid_interface).ok()
}

/// Loads a Candid interface from a local `.did` file.
///
/// Unlike [`get_candid_type`], failures are surfaced to the caller because the
/// user explicitly asked for this file to be used.
pub(crate) fn load_candid_from_file(path: &Path) -> Result<CanisterInterface, anyhow::Error> {
    // Parsed from the path rather than from the text below, so that a `.did`
    // file importing another one still resolves.
    let candid_source = CandidSource::File(path.as_std_path());
    let (type_env, ty) = candid_source
        .load()
        .with_context(|| format!("failed to load Candid interface from {path}"))?;
    let actor =
        ty.ok_or_else(|| anyhow!("Candid file {path} does not declare a service interface"))?;
    Ok(CanisterInterface {
        env: type_env,
        ty: actor,
        source: icp::fs::read_to_string(path)?,
    })
}

pub(crate) struct CanisterInterface {
    pub(crate) env: TypeEnv,
    pub(crate) ty: Type,

    /// The `.did` text this was parsed from. `--sign-only` embeds it in the
    /// message file, since the machine that submits the call has no project to
    /// resolve an interface from.
    pub(crate) source: String,
}

impl CanisterInterface {
    pub(crate) fn from_text(source: String) -> Result<Self, anyhow::Error> {
        let (env, ty) = CandidSource::Text(&source)
            .load()
            .context("failed to parse Candid interface")?;
        let ty = ty.context("Candid interface does not declare a service")?;
        Ok(CanisterInterface { env, ty, source })
    }

    pub(crate) fn methods(&self) -> impl Iterator<Item = &str> {
        let ty = if let TypeInner::Class(_, t) = &*self.ty.0 {
            t
        } else {
            &self.ty
        };
        let TypeInner::Service(methods) = &*ty.0 else {
            unreachable!("check_prog should verify service type")
        };
        methods.iter().map(|(name, _)| name.as_str())
    }
    pub(crate) fn get_method<'a>(&'a self, method_name: &'a str) -> Option<&'a Function> {
        self.env.get_method(&self.ty, method_name).ok()
    }
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
