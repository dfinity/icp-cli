use anyhow::{Context as _, bail};
use candid::{IDLArgs, TypeEnv, types::Function};
use clap::{Args, ValueHint};
use ic_agent::agent::CallResponse;
use icp::context::{Context, IC_ROOT_KEY, NetworkSelection};
use icp::identity::IdentitySelection;
use icp::network::RootKeySpec;
use icp::prelude::*;
use icp::signed_message::{
    CallType, Destination, SUBMISSION_WINDOW, SignedMessage, Validated, WindowState,
    format_timestamp,
};
use std::io::{self, IsTerminal, Read};
use time::{Duration, OffsetDateTime};
use tracing::warn;
use url::Url;

use crate::operations::call_output::{
    CallOutputMode, CanisterInterface, get_candid_type, load_candid_from_file, print_response,
};
use crate::options::NetworkOpt;

/// Submit a message signed on another machine
///
/// Takes a file written by `icp canister call --sign-only`, shows what it
/// contains, submits it, and waits for the reply. No identity is used and none
/// is needed: the message was already signed by whoever composed it, so this
/// machine only has to carry it to the network.
#[derive(Args, Debug)]
pub(crate) struct SendArgs {
    /// The signed message file. `-` reads stdin.
    #[arg(value_hint = ValueHint::FilePath)]
    pub(crate) file: PathBuf,

    /// Where to submit, overriding the network recorded in the file.
    ///
    /// Useful when the signing machine recorded a URL this one cannot reach.
    /// The envelope is signed and carries no URL of its own, so redirecting it
    /// cannot change what executes — only whether it is accepted.
    #[command(flatten)]
    pub(crate) network: NetworkOpt,

    /// Show what the message contains and exit without submitting it.
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Submit without asking for confirmation.
    #[arg(long, short)]
    pub(crate) yes: bool,

    /// Path to a Candid (`.did`) file describing the canister's interface,
    /// overriding the one embedded in the message.
    #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
    pub(crate) candid: Option<PathBuf>,

    /// How to interpret and display the response.
    #[arg(long, short, default_value = "auto")]
    pub(crate) output: CallOutputMode,

    /// Output command results as JSON
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &SendArgs) -> Result<(), anyhow::Error> {
    let message = load(&args.file)?;
    // Everything shown or acted upon below comes from here, decoded out of the
    // signed envelope. The file's own metadata is never trusted for a decision.
    let validated = message
        .validate(OffsetDateTime::now_utc())
        .with_context(|| format!("{} is not a message this can submit", args.file))?;

    // Before anything else, including anything that would touch the network: a
    // message outside its window cannot be submitted, and saying so first makes
    // "not yet valid" read as the state the signer asked for.
    report_window(&validated);
    if !args.dry_run {
        refuse_unsubmittable(&validated)?;
    }

    let (url, root_key) = resolve_network(ctx, args, &message).await?;

    // `--dry-run` is the file-inspection command, so it stays entirely offline:
    // no agent, no root key, and no fetching an interface the file did not carry.
    let agent = match args.dry_run {
        true => None,
        false => {
            let agent = ctx
                .get_agent_for_url(&IdentitySelection::Anonymous, &url)
                .await?;
            apply_root_key(&agent, &root_key).await?;
            Some(agent)
        }
    };

    let interface = resolve_interface(args, &message, &validated, agent.as_ref()).await?;
    let declared_method = interface
        .as_ref()
        .and_then(|i| Some((i.env.clone(), i.get_method(&validated.method)?.clone())));

    print_summary(&message, &validated, &url, declared_method.as_ref());

    if args.dry_run {
        eprintln!("Not submitted: this was a --dry-run.");
        return Ok(());
    }
    let agent = agent.expect("an agent is built whenever the message is submitted");

    if !args.yes && !confirm()? {
        eprintln!("Not submitted.");
        return Ok(());
    }

    let effective_id = message.destination.to_effective_id();
    let response = match validated.call_type {
        CallType::Query => agent
            .query_signed(effective_id, message.request.envelope.clone())
            .await
            .context("the query was rejected")?,
        CallType::Update => {
            let submitted = agent
                .update_signed(effective_id, message.request.envelope.clone())
                .await
                .with_context(|| resubmit_advice(&args.file))?;
            match submitted {
                // The synchronous call path already returned a certified reply.
                CallResponse::Response(reply) => reply,
                // Otherwise await the outcome with the status check the signer
                // pre-signed — which is what lets this machine wait without a key.
                CallResponse::Poll(request_id) => {
                    let status_check = message
                        .request
                        .status_check
                        .clone()
                        .context("an update message must carry a status_check")?;
                    agent
                        .wait_signed(&request_id, effective_id, status_check)
                        .await
                        .map(|(reply, _cert)| reply)
                        .with_context(|| resubmit_advice(&args.file))?
                }
            }
        }
    };

    print_response(&response, args.output, declared_method.as_ref(), args.json)
}

/// Refuses a message that is outside its submission window, naming the window so
/// the operator knows whether to wait or to go back to the signing machine.
fn refuse_unsubmittable(validated: &Validated) -> Result<(), anyhow::Error> {
    match validated.window {
        WindowState::Valid => Ok(()),
        WindowState::Expired => bail!(
            "the submission window closed at {}, so this message can no longer be submitted. \
             It has to be signed again on the machine that holds the key.",
            format_timestamp(validated.valid_until),
        ),
        WindowState::NotYetValid => bail!(
            "the submission window does not open until {} ({} from now). Run this again then; \
             the message stays good until it expires at {}.",
            format_timestamp(validated.valid_from),
            describe_gap(validated.valid_from - OffsetDateTime::now_utc()),
            format_timestamp(validated.valid_until),
        ),
    }
}

/// Reads the message from `path`, or from stdin for `-`.
fn load(path: &Path) -> Result<SignedMessage, anyhow::Error> {
    if path != "-" {
        return Ok(SignedMessage::load(path)?);
    }
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("failed to read the signed message from stdin")?;
    serde_json::from_str(&buf).context("failed to parse the signed message read from stdin")
}

/// Where to submit: the file's network, unless this machine was told otherwise.
async fn resolve_network(
    ctx: &Context,
    args: &SendArgs,
    message: &SignedMessage,
) -> Result<(Url, RootKeySpec), anyhow::Error> {
    let selection: NetworkSelection = args.network.clone().into();
    if selection == NetworkSelection::Default {
        return Ok((
            message.network.url.clone(),
            message.network.root_key.clone(),
        ));
    }
    let network = ctx.get_network(&selection).await?;
    let access = ctx.network.access(&network).await?;
    Ok((access.api_url, RootKeySpec::Explicit(access.root_key)))
}

/// The reply's certificate is verified against this, so it is the one piece of
/// configuration that decides whether an answer can be believed.
async fn apply_root_key(
    agent: &ic_agent::Agent,
    root_key: &RootKeySpec,
) -> Result<(), anyhow::Error> {
    match root_key {
        RootKeySpec::Mainnet => agent.set_root_key(IC_ROOT_KEY.to_vec()),
        RootKeySpec::Explicit(bytes) => agent.set_root_key(bytes.clone()),
        RootKeySpec::Fetch => {
            warn!(
                "fetching the root key from the network; its provenance is not verified \
                 (trust-on-first-use), so the reply's certificate proves less than a pinned key would"
            );
            agent
                .fetch_root_key()
                .await
                .context("failed to fetch the root key")?;
        }
    }
    Ok(())
}

/// `--candid` → the interface embedded by the signer → what the canister
/// publishes → nothing.
///
/// The embedded interface sorts above the fetch deliberately: it is the one the
/// signer used to *encode* the argument, so decoding with it is self-consistent,
/// needs no round trip, and works for a canister exposing no metadata. The fetch
/// stays a real fallback because this machine, unlike the signer, is online —
/// and is skipped entirely when there is no agent, i.e. under `--dry-run`.
async fn resolve_interface(
    args: &SendArgs,
    message: &SignedMessage,
    validated: &Validated,
    agent: Option<&ic_agent::Agent>,
) -> Result<Option<CanisterInterface>, anyhow::Error> {
    if let Some(path) = &args.candid {
        return Ok(Some(load_candid_from_file(path)?));
    }
    if let Some(embedded) = &message.candid {
        match CanisterInterface::from_text(embedded.clone()) {
            Ok(interface) => return Ok(Some(interface)),
            // Display only, so a stale or broken interface degrades rather than
            // stopping a message that is otherwise perfectly submittable.
            Err(e) => warn!("ignoring the interface embedded in the message: {e}"),
        }
    }
    match agent {
        Some(agent) => Ok(get_candid_type(agent, validated.canister_id).await),
        None => Ok(None),
    }
}

/// Says where now falls in the window before anything else happens, so "not yet
/// valid" reads as the state the signer asked for rather than as a puzzle.
fn report_window(validated: &Validated) {
    let now = OffsetDateTime::now_utc();
    match validated.window {
        WindowState::NotYetValid => eprintln!(
            "Not yet submittable. The window opens at {} — {} from now — and closes at {}.",
            format_timestamp(validated.valid_from),
            describe_gap(validated.valid_from - now),
            format_timestamp(validated.valid_until),
        ),
        WindowState::Valid => eprintln!(
            "Submittable for another {}, until {}.",
            describe_gap(validated.valid_until - now),
            format_timestamp(validated.valid_until),
        ),
        WindowState::Expired => eprintln!(
            "Expired. The window closed at {}, {} ago.",
            format_timestamp(validated.valid_until),
            describe_gap(now - validated.valid_until),
        ),
    }
    // The window is computed from the signing machine's clock, and an offline
    // machine is exactly the kind that drifts.
    if matches!(
        validated.window,
        WindowState::NotYetValid | WindowState::Expired
    ) && (validated.valid_from - now).abs() < SUBMISSION_WINDOW
    {
        eprintln!(
            "It is only just outside the window, which usually means the signing machine's \
             clock is off rather than that you are early or late."
        );
    }
}

fn print_summary(
    message: &SignedMessage,
    validated: &Validated,
    url: &Url,
    declared_method: Option<&(TypeEnv, Function)>,
) {
    eprintln!();
    eprintln!("  Sender:      {}", validated.sender);
    eprintln!("  Canister:    {}", validated.canister_id);
    eprintln!(
        "  Method:      {} ({})",
        validated.method,
        validated.call_type.as_str()
    );
    eprintln!(
        "  Argument:    {}",
        render_argument(&validated.arg, declared_method)
    );
    eprintln!("  Network:     {url}");
    // Unauthenticated, and for a subnet-scoped canister creation the destination
    // *is* the choice of subnet — so it is shown rather than assumed.
    match message.destination {
        Destination::Canister(id) => eprintln!("  Destination: canister {id}"),
        Destination::Subnet(id) => eprintln!("  Destination: subnet {id}"),
    }
    eprintln!();
}

/// The argument decoded, or hex if there is no interface to decode it with.
/// A prompt showing a hex blob is not something anyone can review.
fn render_argument(arg: &[u8], declared_method: Option<&(TypeEnv, Function)>) -> String {
    let decoded = match declared_method {
        Some((env, func)) => IDLArgs::from_bytes_with_types(arg, env, &func.args).ok(),
        None => IDLArgs::from_bytes(arg).ok(),
    };
    match decoded {
        Some(args) => format!("{args}"),
        None => format!("{} (hex, could not decode)", hex::encode(arg)),
    }
}

fn confirm() -> Result<bool, anyhow::Error> {
    // A courier is expected to be scripted, and blocking on a prompt nobody can
    // answer is worse than submitting a message its author already reviewed.
    if !io::stdin().is_terminal() {
        return Ok(true);
    }
    dialoguer::Confirm::new()
        .with_prompt("Submit this message?")
        .default(false)
        .interact()
        .context("failed to read confirmation")
}

/// What to do when a submission fails after the message may already have been
/// sent. Re-running is safe; re-signing is not.
fn resubmit_advice(file: &Path) -> String {
    format!(
        "the message may already have reached the network. Re-run `icp message send {file}` on \
         the same file — the request id is a hash of the signed content, so resubmitting the \
         identical message is de-duplicated by the IC and cannot execute twice. Do NOT sign it \
         again: a new signature means a new expiry, hence a different request id, which is NOT \
         de-duplicated. Two limits worth knowing: once the reply is pruned from the ingress \
         history the outcome can no longer be recovered (the call still executed), and once the \
         window closes the message cannot be submitted at all"
    )
}

/// A rough, readable gap, for telling someone how long they have or how long to wait.
fn describe_gap(gap: Duration) -> String {
    let seconds = gap.whole_seconds().abs();
    match seconds {
        0..=90 => format!("{seconds}s"),
        91..=5400 => format!("{}m", (seconds + 30) / 60),
        _ => format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60),
    }
}
