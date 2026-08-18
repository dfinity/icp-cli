use anyhow::{Context as _, anyhow, bail};
use candid::types::{Type, TypeInner};
use candid::{IDLArgs, Principal, TypeEnv, types::Function};
use candid_parser::assist;
use candid_parser::parse_idl_args;
use candid_parser::utils::CandidSource;
use clap::{Args, ValueEnum, ValueHint};
use dialoguer::console::Term;
use ic_agent::Agent;
use ic_agent::agent::EffectiveId;
use icp::context::{Context, EnvironmentSelection, NetworkSelection};
use icp::manifest::ArgsFormat;
use icp::network::{Configuration as NetworkConfiguration, RootKeySpec};
use icp::parsers::{CyclesAmount, DurationAmount};
use icp::prelude::*;
use icp::signed_message::{
    self, CallType, Destination, Request, SignedMessage, Summary, WindowState,
};
use serde::Serialize;
use std::io::{self, Write};
use std::str::FromStr;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{error, warn};
use url::Url;

use crate::{
    commands::args::{self, load_args},
    operations::misc::fetch_canister_metadata,
    operations::proxy::update_or_proxy_raw,
    operations::wasm::extract_candid_service,
};

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
    #[arg(long, conflicts_with = "args", value_hint = ValueHint::FilePath)]
    pub(crate) args_file: Option<PathBuf>,

    /// Format of the call arguments.
    #[arg(long, default_value = "candid")]
    pub(crate) args_format: ArgsFormat,

    /// Path to a Candid (`.did`) file describing the canister's interface.
    ///
    /// When set, this interface is used to assist method selection, build
    /// arguments, and decode the response, instead of fetching the canister's
    /// Candid interface from the network.
    #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
    pub(crate) candid: Option<PathBuf>,

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

    /// Output command results as JSON
    #[arg(long)]
    pub(crate) json: bool,

    /// Sign the call and write it to FILE instead of submitting it, so it can be
    /// submitted later from a machine that has network access but not your key.
    /// `-` writes to stdout.
    ///
    /// Nothing is sent, and nothing is fetched: the interface comes from
    /// `--candid` or the local build artifact rather than from the canister, so
    /// this works with no network at all. `--root-key` must name a key rather
    /// than `fetch`, and `--proxy` is not supported.
    #[arg(long, value_name = "FILE", conflicts_with = "proxy", value_hint = ValueHint::FilePath)]
    pub(crate) sign_only: Option<PathBuf>,

    /// When the signed message's five-minute submission window opens: a duration
    /// from now (`55m`, `2h`) or an RFC 3339 timestamp
    /// (`2026-08-17T10:07:00Z`). Defaults to now.
    ///
    /// The window is always five minutes wide — the IC will not accept an
    /// ingress message expiring further ahead than that — so this places it
    /// rather than sizing it. It is rounded down to the whole minute, and so may
    /// open up to 59 seconds earlier than asked; the file records the window it
    /// actually got.
    #[arg(long, value_name = "WHEN", requires = "sign_only")]
    pub(crate) valid_from: Option<ValidFrom>,
}

/// When a signed message's five-minute submission window opens.
#[derive(Clone, Debug)]
pub(crate) enum ValidFrom {
    /// A duration from now.
    In(time::Duration),
    /// An absolute instant.
    At(OffsetDateTime),
}

impl FromStr for ValidFrom {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(at) = OffsetDateTime::parse(s, &Rfc3339) {
            return Ok(ValidFrom::At(at));
        }
        let seconds = DurationAmount::from_str(s)
            .ok()
            .map(|d| d.get())
            .and_then(|secs| i64::try_from(secs).ok());
        match seconds {
            Some(seconds) => Ok(ValidFrom::In(time::Duration::seconds(seconds))),
            None => Err(format!(
                "'{s}' is neither a duration ('55m', '2h') nor an RFC 3339 timestamp \
                 ('2026-08-17T10:07:00Z')"
            )),
        }
    }
}

pub(crate) async fn exec(ctx: &Context, args: &CallArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    // Signing is meant to work on a machine with no network, so no agent is
    // built up front: the one used to sign is created last, once the submission
    // window it has to expire on is known.
    let agent = match args.sign_only {
        Some(_) => None,
        None => Some(
            ctx.get_agent(
                &selections.identity,
                &selections.network,
                &selections.environment,
            )
            .await?,
        ),
    };
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let candid_types = match (&args.candid, &agent) {
        (Some(path), _) => Some(load_candid_from_file(path)?),
        (None, Some(agent)) => get_candid_type(agent, cid).await,
        // Fetching `candid:service` is a network round trip, so signing falls
        // back to the interface of whatever this project last built.
        (None, None) => local_candid_type(ctx, &selections.canister).await,
    };

    let method = if let Some(method) = &args.method {
        method.clone()
    } else if let Some(interface) = &candid_types {
        // Interactive method selection using candid assist
        let methods: Vec<&str> = interface.methods().collect();
        if methods.is_empty() {
            bail!("the canister's Candid interface has no methods");
        }
        let selection = dialoguer::Select::new()
            .with_prompt("Select a method to call")
            .items(&methods)
            .default(0)
            .interact()?;
        methods[selection].to_string()
    } else {
        bail!(
            "method name was not provided and no Candid interface is available to assist method selection"
        );
    };
    let declared_method = candid_types
        .as_ref()
        .and_then(|i| Some((i.env.clone(), i.get_method(&method)?.clone())));
    enum ResolvedArgs {
        Candid(IDLArgs),
        Bytes(Vec<u8>),
    }

    let resolved_args = match load_args(
        args.args.as_deref(),
        args.args_file.as_ref(),
        &args.args_format,
        "a positional argument",
    )? {
        None => None,
        Some(icp::InitArgs::Binary(bytes)) => Some(ResolvedArgs::Bytes(bytes)),
        Some(icp::InitArgs::Text {
            content,
            format: ArgsFormat::Candid,
        }) => Some(ResolvedArgs::Candid(
            parse_idl_args(&content).context("failed to parse Candid arguments")?,
        )),
        Some(icp::InitArgs::Text {
            content,
            format: ArgsFormat::Hex,
        }) => Some(ResolvedArgs::Bytes(
            hex::decode(&content).context("failed to decode hex arguments")?,
        )),
        Some(icp::InitArgs::Text {
            format: ArgsFormat::Bin,
            ..
        }) => {
            unreachable!("load_args rejects bin format for inline values")
        }
    };

    let arg_bytes = match (&declared_method, resolved_args) {
        (_, None) if args.args_format != ArgsFormat::Candid => {
            bail!("arguments must be provided when --args-format is not candid");
        }
        (None, None) => bail!(
            "arguments were not provided and no Candid interface is available to assist building arguments"
        ),
        (None, Some(ResolvedArgs::Bytes(bytes))) => bytes,
        (None, Some(ResolvedArgs::Candid(arguments))) => {
            warn!("no Candid interface is available, serializing arguments with inferred types.");
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
                .to_bytes_with_types(type_env, &func.args)
                .context("failed to serialize candid arguments with specific types")?
        }
        (Some(_), Some(ResolvedArgs::Bytes(bytes))) => bytes,
        (Some((type_env, func)), Some(ResolvedArgs::Candid(arguments))) => arguments
            .to_bytes_with_types(type_env, &func.args)
            .context("failed to serialize candid arguments with specific types")?,
    };

    // Preemptive check: error if Candid shows this is an update method
    if args.query
        && let Some((_, func)) = &declared_method
        && !func.is_query()
    {
        bail!(
            "`{method}` is an update method, not a query method. \
             Run the command without `--query`.",
        );
    }

    if let Some(out) = &args.sign_only {
        return sign_only(
            ctx,
            args,
            out,
            cid,
            &method,
            arg_bytes,
            candid_types.as_ref().map(|i| i.source.as_str()),
        )
        .await;
    }

    let agent = agent.expect("an agent is built whenever the call is submitted");
    let res = if args.query {
        agent
            .query(&cid, &method)
            .with_arg(arg_bytes)
            .call()
            .await?
    } else {
        update_or_proxy_raw(
            &agent,
            cid,
            &method,
            arg_bytes,
            args.proxy,
            None,
            args.cycles.get(),
        )
        .await?
    };

    let mut term = Term::buffered_stdout();
    let decoded = decode_response(&res, args.output, declared_method.as_ref());

    if args.json {
        let envelope = JsonCallResponse::build(&res, decoded.as_ref().ok());
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
            Decoded::Bytes => writeln!(term, "{}", hex::encode(&res))?,
        }
    }

    // term is buffered; this single flush covers all output paths (json and non-json).
    term.flush()?;

    Ok(())
}

/// Signs the call and writes it out for another machine to submit, instead of
/// submitting it here.
///
/// Nothing in this path reaches the network. The interface has already been
/// resolved without one, the root key is recorded rather than fetched, and the
/// agent exists only to hold the key and the expiry.
async fn sign_only(
    ctx: &Context,
    args: &CallArgs,
    out: &Path,
    cid: Principal,
    method: &str,
    arg_bytes: Vec<u8>,
    interface: Option<&str>,
) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let (url, root_key) =
        resolve_network_offline(ctx, &selections.network, &selections.environment).await?;

    let now = OffsetDateTime::now_utc();
    // Checked throughout: `--valid-from` accepts any duration that fits a `u64`
    // of seconds, which reaches far past the last representable instant, and
    // `OffsetDateTime`'s `+` panics rather than saturating.
    let out_of_range = || {
        anyhow!(
            "`--valid-from` is too far from now to place a submission window in representable time"
        )
    };
    let opens_at = match &args.valid_from {
        Some(ValidFrom::At(at)) => *at,
        Some(ValidFrom::In(duration)) => now.checked_add(*duration).ok_or_else(out_of_range)?,
        None => now,
    };
    let valid_until = opens_at
        .checked_add(signed_message::SUBMISSION_WINDOW)
        .map(floor_to_minute)
        .ok_or_else(out_of_range)?;
    let valid_from = valid_until
        .checked_sub(signed_message::SUBMISSION_WINDOW)
        .ok_or_else(out_of_range)?;
    if valid_until <= now {
        bail!(
            "`--valid-from` puts the submission window at {} to {}, which has already closed",
            signed_message::format_timestamp(valid_from),
            signed_message::format_timestamp(valid_until),
        );
    }

    let agent = ctx
        .get_agent_for_signing(&selections.identity, &url, valid_until)
        .await?;
    let sender = agent
        .get_principal()
        .map_err(|e| anyhow!("failed to determine the signing identity's principal: {e}"))?;

    let request = if args.query {
        let signed = agent
            .query(&cid, method)
            .with_arg(arg_bytes.clone())
            .expire_at(valid_until)
            .sign()
            .context("failed to sign the query")?;
        Request {
            call_type: CallType::Query,
            envelope: signed.signed_query,
            // A query answers immediately, so there is nothing to poll for.
            request_id: None,
            status_check: None,
        }
    } else {
        let signed = agent
            .update(&cid, method)
            .with_arg(arg_bytes.clone())
            .expire_at(valid_until)
            .sign()
            .context("failed to sign the call")?;
        let status_check = agent
            .sign_request_status(EffectiveId::Canister(cid), signed.request_id)
            .context("failed to sign the request-status check")?;

        // Both envelopes have to expire at the same instant: a window is
        // `[expiry - 5min, expiry]`, so a status check with an expiry of its own
        // would be waiting on the call from a different window. It gets its
        // expiry from the agent, which is why the agent's was pinned above.
        anyhow::ensure!(
            status_check.ingress_expiry == signed.ingress_expiry,
            "the call and its status check landed in different submission windows \
             ({} vs {}); this is a bug",
            signed.ingress_expiry,
            status_check.ingress_expiry,
        );

        Request {
            call_type: CallType::Update,
            envelope: signed.signed_update,
            request_id: Some(signed.request_id.to_string()),
            status_check: Some(status_check.signed_request_status),
        }
    };
    let call_type = request.call_type;

    let message = SignedMessage {
        format: signed_message::FORMAT.to_string(),
        version: signed_message::VERSION,
        request,
        network: signed_message::Network { url, root_key },
        // Routed by the target's own canister id, which is what `call` does
        // online too — it passes no effective canister id to
        // `update_or_proxy_raw`. So a management-canister call records
        // `aaaaa-aa` and is misrouted here exactly as it already is online. A
        // subnet destination is legal in the format, but nothing produces one yet.
        destination: Destination::Canister(cid),
        candid: interface.map(str::to_owned),
        summary: Summary {
            sender,
            canister_id: cid,
            method: method.to_owned(),
            arg: arg_bytes,
            signed_at: signed_message::format_timestamp(now),
            valid_from: signed_message::format_timestamp(valid_from),
            valid_until: signed_message::format_timestamp(valid_until),
        },
    };

    // Refuse to hand over a file we would not accept back. Signing is the last
    // thing the air-gapped machine does, so a mistake found on the other side is
    // found too late. The clock is re-read rather than reused from above:
    // unlocking the identity and signing on a hardware token both take time.
    let validated = message
        .validate(OffsetDateTime::now_utc())
        .context("the signed message failed its own validation")?;
    anyhow::ensure!(
        validated.window != WindowState::Expired,
        "the submission window closed at {} while the message was being signed, \
         so it could no longer be submitted; nothing was written",
        signed_message::format_timestamp(valid_until),
    );

    if out == "-" {
        let mut stdout = io::stdout();
        writeln!(stdout, "{}", message.to_json()?)?;
        stdout.flush()?;
    } else {
        message.save(out)?;
    }

    eprintln!(
        "Signed a {} call to '{method}' on {cid}, as {sender}.",
        call_type.as_str(),
    );
    eprintln!(
        "It can be submitted between {} and {} — a five-minute window.",
        signed_message::format_timestamp(valid_from),
        signed_message::format_timestamp(valid_until),
    );
    if out != "-" {
        eprintln!("Written to {out}.");
    }
    if interface.is_none() {
        warn!(
            "no Candid interface was available, so the message carries none; \
             whoever submits it will see the argument and reply undecoded unless they supply one"
        );
    }

    Ok(())
}

/// Resolves where a signed message says to submit itself, without touching the
/// network.
///
/// The signing machine may be air-gapped, and the only thing `call` needs a root
/// key for is verifying a reply it will never see — so the key is recorded for
/// the submitting machine rather than resolved here, and a network configured to
/// fetch one is rejected outright instead of hanging until it times out.
async fn resolve_network_offline(
    ctx: &Context,
    network: &NetworkSelection,
    environment: &EnvironmentSelection,
) -> Result<(Url, RootKeySpec), anyhow::Error> {
    let net = match (environment, network) {
        (EnvironmentSelection::Named(_), NetworkSelection::Named(_))
        | (EnvironmentSelection::Named(_), NetworkSelection::Url(_, _)) => {
            bail!("You can't specify both an environment and a network")
        }
        (_, NetworkSelection::Default) => ctx.get_environment(environment).await?.network,
        (EnvironmentSelection::Default, _) => ctx.get_network(network).await?,
    };

    match net.configuration.clone() {
        NetworkConfiguration::Connected { connected } => {
            if connected.root_key == RootKeySpec::Fetch {
                bail!(
                    "network '{}' fetches its root key from {}, which `--sign-only` cannot do — \
                     signing must work with no network. Name the key instead: `--root-key mainnet`, \
                     or a hex-encoded root key.",
                    net.name,
                    connected.api_url,
                );
            }
            Ok((connected.api_url, connected.root_key))
        }
        // A managed network's root key comes out of the descriptor this machine
        // wrote when it started the network: a local file, not a request.
        NetworkConfiguration::Managed { .. } => {
            let access = ctx.network.access(&net).await?;
            Ok((access.api_url, RootKeySpec::Explicit(access.root_key)))
        }
    }
}

/// Rounds an ingress expiry down to a whole minute.
///
/// `ic-agent` truncates the seconds off any ingress expiry it derives itself,
/// which is how the pre-signed status check gets one. Choosing a minute-aligned
/// expiry for the call makes that truncation a no-op, so the two envelopes name
/// the same instant and share one window.
fn floor_to_minute(t: OffsetDateTime) -> OffsetDateTime {
    t.replace_nanosecond(0)
        .and_then(|t| t.replace_second(0))
        .expect("0 is a valid second and nanosecond")
}

/// A response decoded according to the requested `CallOutputMode`.
enum Decoded {
    Candid(IDLArgs),
    Text(String),
    /// No decoding was attempted or all attempts failed; emit raw bytes as hex.
    Bytes,
}

fn decode_response(
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
async fn get_candid_type(agent: &Agent, canister_id: Principal) -> Option<CanisterInterface> {
    let candid_interface = fetch_canister_metadata(agent, canister_id, "candid:service").await?;
    CanisterInterface::from_text(candid_interface).ok()
}

/// Gets the Candid interface a project canister was last built with, from the
/// `candid:service` metadata of its build artifact.
///
/// Best effort, and offline: it stands in for [`get_candid_type`] when the
/// canister cannot be reached, so a canister that was never built, or was named
/// by principal rather than by name, simply yields nothing.
async fn local_candid_type(
    ctx: &Context,
    canister: &icp::context::CanisterSelection,
) -> Option<CanisterInterface> {
    let icp::context::CanisterSelection::Named(name) = canister else {
        return None;
    };
    let wasm = ctx.artifacts.lookup(name).await.ok()?;
    CanisterInterface::from_text(extract_candid_service(&wasm)?).ok()
}

/// Loads a Candid interface from a local `.did` file.
///
/// Unlike [`get_candid_type`], failures are surfaced to the caller because the
/// user explicitly asked for this file to be used.
fn load_candid_from_file(path: &Path) -> Result<CanisterInterface, anyhow::Error> {
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

struct CanisterInterface {
    env: TypeEnv,
    ty: Type,

    /// The `.did` text this was parsed from. `--sign-only` embeds it in the
    /// message file, since the machine that submits the call has no project to
    /// resolve an interface from.
    source: String,
}

impl CanisterInterface {
    fn from_text(source: String) -> Result<Self, anyhow::Error> {
        let (env, ty) = CandidSource::Text(&source)
            .load()
            .context("failed to parse Candid interface")?;
        let ty = ty.context("Candid interface does not declare a service")?;
        Ok(CanisterInterface { env, ty, source })
    }

    fn methods(&self) -> impl Iterator<Item = &str> {
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
    fn get_method<'a>(&'a self, method_name: &'a str) -> Option<&'a Function> {
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
