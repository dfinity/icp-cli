use anyhow::bail;
use candid::Principal;
use clap::Args;
use clap_complete::ArgValueCandidates;
use ic_agent::{Agent, AgentError};
use icp::operations::deploy::{DeployParams, DeployReport, deploy, resolve_targets};
use icp::parsers::CyclesAmount;
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection},
    identity::IdentitySelection,
    network::Configuration as NetworkConfiguration,
};
use icp_canister_interfaces::candid_ui::MAINNET_CANDID_UI_CID;
use serde::Serialize;
use tracing::info;

use crate::options::EnvironmentOpt;
use crate::{
    commands::{args::ArgsOpt, canister::create},
    options::{IdentityOpt, arg_struct_change_help},
    render::rendered,
};

/// Deploy a project to an environment
#[derive(Args, Debug)]
#[command(after_long_help = "\
When deploying a single canister, you can pass arguments to the install call
using --args or --args-file:

    # Pass inline Candid arguments
    icp deploy my_canister --args '(42 : nat)'

    # Pass arguments from a file
    icp deploy my_canister --args-file ./args.did

    # Pass raw bytes
    icp deploy my_canister --args-file ./args.bin --args-format bin
")]
pub(crate) struct DeployArgs {
    /// Canister names
    #[arg(add = ArgValueCandidates::new(crate::complete::canisters))]
    pub(crate) names: Vec<String>,

    /// Specifies the mode of canister installation.
    #[arg(long, short, default_value = "auto", value_parser = ["auto", "install", "reinstall", "upgrade"])]
    pub(crate) mode: String,

    /// The subnet to use for the canisters being deployed.
    #[clap(long, conflicts_with = "proxy")]
    pub(crate) subnet: Option<Principal>,

    /// Principal of a proxy canister to route management canister calls through.
    #[arg(long, conflicts_with = "subnet")]
    pub(crate) proxy: Option<Principal>,

    /// One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple.
    #[arg(long)]
    pub(crate) controller: Vec<Principal>,

    /// Cycles to fund canister creation.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, default_value_t = CyclesAmount::from(create::DEFAULT_CANISTER_CYCLES))]
    pub(crate) cycles: CyclesAmount,

    /// If any canisters do not exist, error instead of creating them.
    #[arg(long, conflicts_with_all = ["subnet", "cycles"])]
    pub(crate) no_create: bool,

    /// Skip confirmation prompts, including the Candid interface compatibility check.
    #[arg(long, short)]
    pub(crate) yes: bool,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,

    #[command(flatten)]
    pub(crate) environment: DeployEnvironmentOpt,

    /// Output command results as JSON
    #[arg(long)]
    pub(crate) json: bool,

    /// Arguments to pass to the canister on install or upgrade.
    /// Only valid when deploying a single canister. Takes priority over `init_args` and
    /// `upgrade_args` in the manifest.
    #[command(flatten)]
    pub(crate) args_opt: ArgsOpt,
}

arg_struct_change_help!(
    EnvironmentOpt => DeployEnvironmentOpt,
    arg = "environment",
    help = "Override the environment to build for and deploy to. By default, the local environment is used"
);

pub(crate) async fn exec(ctx: &Context, args: &DeployArgs) -> Result<(), anyhow::Error> {
    let environment_selection: EnvironmentSelection = args.environment.0.clone().into();
    let identity_selection: IdentitySelection = args.identity.clone().into();

    let canisters = resolve_targets(ctx, &environment_selection, &args.names).await?;

    // Skip doing any work if no canisters are targeted. Say so: an environment
    // whose `canisters` lists leave out everything in scope is otherwise an
    // `icp deploy` that succeeds in silence.
    if canisters.is_empty() {
        info!(
            "Environment '{}' contains no canisters to deploy",
            environment_selection.name()
        );
        return Ok(());
    }

    if args.args_opt.is_some() && canisters.len() != 1 {
        bail!("--args and --args-file can only be used when deploying a single canister");
    }

    let params = DeployParams {
        environment: environment_selection.clone(),
        identity: identity_selection.clone(),
        canisters: canisters.clone(),
        mode: args.mode.clone(),
        subnet: args.subnet,
        proxy: args.proxy,
        cycles: args.cycles.get(),
        no_create: args.no_create,
        yes: args.yes,
        args: args.args_opt.resolve_bytes()?,
    };

    // One reporter for the whole deploy: every phase and every canister lands
    // on the same stream, so the renderer can order the run without the
    // command having to await it phase by phase.
    let mut report = DeployReport::default();
    let result = rendered(ctx.debug, async |reporter| {
        deploy(ctx, &params, reporter, &mut report).await
    })
    .await;

    // Terminal output is the command's job. The operation reports what it did;
    // this decides how to say it. Printed before the result is unwrapped: a
    // canister created before a later phase failed still exists, and the user
    // needs its id either way.
    if !args.json {
        for (name, id) in &report.created {
            println!("Created canister {name} with ID {id}");
        }
    }
    result?;

    let agent = ctx
        .get_agent_for_env(&identity_selection, &environment_selection)
        .await?;
    print_canister_urls(ctx, &environment_selection, agent, &canisters, args.json).await?;

    Ok(())
}

/// Checks whether a canister speaks the HTTP gateway protocol — i.e. exposes an
/// `http_request` query method — so we print its gateway (frontend) URL instead
/// of a Candid UI URL.
///
/// The probe deliberately errs toward false positives over false negatives: any
/// canister that *has* an `http_request` method should get a frontend URL, even
/// when we can't hand it a valid request. So rather than sending a crafted
/// request payload — whose Candid type must match the canister's exactly, and
/// which silently yields a false negative when it doesn't (the bug this
/// replaces) — we send a zero-argument call and look only at *why* it fails.
///
/// The replica reports a missing method as `CanisterMethodNotFound` (error code
/// `IC0536`). Any other outcome means the method exists and ran: a reply (from a
/// zero-argument `http_request`), or — far more likely — a trap/decode failure
/// (`IC0502`/`IC0503`/`IC0504`) from feeding a normal single-argument
/// `http_request` the wrong number of arguments. Note the reject *code* can't
/// tell these apart: method-not-found and a trap are both `CanisterError` (5),
/// so only the `error_code` string distinguishes them. Transport/other errors
/// are inconclusive and, per the false-positive bias, also count as "has
/// `http_request`".
async fn has_http_request(agent: &Agent, canister_id: Principal) -> bool {
    // A valid Candid encoding of zero arguments (`DIDL\0\0`) — *not* raw empty
    // bytes, which are not a well-formed Candid message. This lets a genuine
    // zero-argument `http_request` reply, while a single-argument one still
    // fails to decode and traps; either way the method exists.
    let empty_args = candid::encode_args(()).expect("encoding () never fails");
    let result = agent
        .query(&canister_id, "http_request")
        .with_arg(empty_args)
        .call()
        .await;

    match result {
        Ok(_) => true,
        Err(err) => !is_method_not_found(&err),
    }
}

/// True when a query error is the replica's definitive "this method does not
/// exist" signal — `CanisterMethodNotFound` (`IC0536`).
///
/// When the replica populates `error_code` we trust it exclusively: a trap or
/// decode failure (`IC0503`/`IC0504`) is *not* method-absent, even if its
/// message happens to mention a missing method — treating it as absent would
/// reintroduce the very false negative this probe exists to avoid. Only when
/// `error_code` is absent (older replicas) do we fall back to the message, and
/// then only when it names `http_request`, so a nested "no such method" bubbled
/// up from an existing handler isn't mistaken for `http_request` being absent.
fn is_method_not_found(err: &AgentError) -> bool {
    let reject = match err {
        AgentError::CertifiedReject { reject, .. }
        | AgentError::UncertifiedReject { reject, .. } => reject,
        _ => return false,
    };
    match reject.error_code.as_deref() {
        Some(code) => code == "IC0536",
        None => {
            reject.reject_message.contains("has no query method")
                && reject.reject_message.contains("http_request")
        }
    }
}

/// Prints URLs for deployed canisters
async fn print_canister_urls(
    ctx: &Context,
    environment_selection: &EnvironmentSelection,
    agent: Agent,
    canister_names: &[String],
    json: bool,
) -> Result<(), anyhow::Error> {
    use icp::network::custom_domains::{canister_gateway_url, gateway_domain};

    let env = ctx.get_environment(environment_selection).await?;

    // Get the network URL
    let (http_gateway_url, has_friendly) = match &env.network.configuration {
        NetworkConfiguration::Managed { managed: _ } => {
            let access = ctx.network.access(&env.network).await?;
            (access.http_gateway_url.clone(), access.use_friendly_domains)
        }
        NetworkConfiguration::Connected { connected } => {
            (connected.http_gateway_url.clone(), false)
        }
    };

    let mut json_canisters = Vec::new();
    // Human-readable output is grouped by kind so the two URL flavors don't
    // interleave: frontends (something to open in a browser) first, then
    // backends (a Candid UI to poke the interface).
    let mut frontend_lines: Vec<String> = Vec::new();
    let mut backend_lines: Vec<String> = Vec::new();
    // Only populated when the network exposes no gateway at all — there is then
    // nothing to group, so these render as a flat list under the header.
    let mut no_gateway_lines: Vec<String> = Vec::new();

    for name in canister_names {
        let canister_id = match ctx
            .get_canister_id_for_env(
                &CanisterSelection::Named(name.clone()),
                environment_selection,
            )
            .await
        {
            Ok(id) => id,
            Err(_) => continue,
        };

        let Some(http_gateway_url) = &http_gateway_url else {
            json_canisters.push(JsonDeployedCanister {
                name: name.clone(),
                canister_id,
                url: None,
            });
            no_gateway_lines.push(format!(
                "  {name}: {canister_id} (No gateway URL available)"
            ));
            continue;
        };

        if has_http_request(&agent, canister_id).await {
            // A canister carries one friendly name normally, or several when
            // it's a de-duplicated shared dependency canister reached via
            // multiple alias chains — print one URL for each. Fall back to a
            // single principal URL when friendly domains are off or no
            // friendly name is known.
            let env_name = environment_selection.name();
            let friendly_names: Vec<String> = if has_friendly {
                env.canisters
                    .get(name)
                    .map(|(_, c)| c.friendly_names.clone())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let urls = if friendly_names.is_empty() {
                vec![canister_gateway_url(http_gateway_url, canister_id, None)]
            } else {
                friendly_names
                    .iter()
                    .map(|fname| {
                        canister_gateway_url(
                            http_gateway_url,
                            canister_id,
                            Some((fname.as_str(), env_name)),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            for canister_url in &urls {
                json_canisters.push(JsonDeployedCanister {
                    name: name.clone(),
                    canister_id,
                    url: Some(canister_url.to_string()),
                });
                frontend_lines.push(format!("  {name}: {canister_url}"));
            }
        } else {
            // No http_request: offer the Candid UI URL instead.
            let url = if let Some(ui_id) = get_candid_ui_id(ctx, environment_selection).await {
                let domain = gateway_domain(http_gateway_url);
                let mut candid_url = canister_gateway_url(http_gateway_url, ui_id, None);
                if domain.is_some() {
                    candid_url.set_query(Some(&format!("id={canister_id}")));
                } else {
                    candid_url.set_query(Some(&format!("canisterId={ui_id}&id={canister_id}")));
                }
                backend_lines.push(format!("  {name}: {candid_url}"));
                Some(candid_url.to_string())
            } else {
                backend_lines.push(format!("  {name}: {canister_id} (Candid UI not available)"));
                None
            };
            json_canisters.push(JsonDeployedCanister {
                name: name.clone(),
                canister_id,
                url,
            });
        }
    }

    if json {
        serde_json::to_writer(
            std::io::stdout(),
            &JsonDeploy {
                canisters: json_canisters,
            },
        )?;
        return Ok(());
    }

    println!("Deployed canisters:");
    if !frontend_lines.is_empty() {
        println!();
        println!("Frontends (serving http_request):");
        for line in &frontend_lines {
            println!("{line}");
        }
    }
    if !backend_lines.is_empty() {
        println!();
        println!("Backends (Candid UI):");
        for line in &backend_lines {
            println!("{line}");
        }
    }
    for line in &no_gateway_lines {
        println!("{line}");
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonDeploy {
    canisters: Vec<JsonDeployedCanister>,
}

#[derive(Serialize)]
struct JsonDeployedCanister {
    name: String,
    canister_id: Principal,
    url: Option<String>,
}

/// Gets the Candid UI canister ID for the network
/// Returns None if the Candid UI ID cannot be determined
async fn get_candid_ui_id(
    ctx: &Context,
    environment_selection: &EnvironmentSelection,
) -> Option<Principal> {
    let env = ctx.get_environment(environment_selection).await.ok()?;

    match &env.network.configuration {
        NetworkConfiguration::Managed { managed: _ } => {
            // Try to get the candid UI ID from the network descriptor
            let nd = ctx.network.get_network_directory(&env.network).ok()?;
            if let Ok(Some(desc)) = nd.load_network_descriptor().await
                && let Some(candid_ui) = desc.candid_ui_canister_id
            {
                return Some(candid_ui);
            }
            // No Candid UI available for this managed network
            None
        }
        NetworkConfiguration::Connected { .. } => {
            // For connected networks, use the mainnet Candid UI
            Some(MAINNET_CANDID_UI_CID)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ic_agent::agent::{RejectCode, RejectResponse};

    fn reject(error_code: Option<&str>, reject_message: &str) -> AgentError {
        AgentError::UncertifiedReject {
            reject: RejectResponse {
                // Both a missing method and a trap surface as `CanisterError`,
                // so the reject code is intentionally the same across cases —
                // only `error_code`/message distinguishes them.
                reject_code: RejectCode::CanisterError,
                reject_message: reject_message.to_string(),
                error_code: error_code.map(String::from),
            },
            operation: None,
        }
    }

    #[test]
    fn method_not_found_detected_by_error_code() {
        // IC0536 = CanisterMethodNotFound: the method genuinely does not exist.
        let err = reject(
            Some("IC0536"),
            "Canister abc has no query method 'http_request'",
        );
        assert!(is_method_not_found(&err));
    }

    #[test]
    fn method_not_found_detected_by_message_when_error_code_absent() {
        // Replicas that don't populate `error_code` still carry the message,
        // which names the missing method (`http_request`, since we query it).
        let query = reject(None, "Canister abc has no query method 'http_request'");
        assert!(is_method_not_found(&query));
    }

    #[test]
    fn present_error_code_wins_over_misleading_message() {
        // An existing `http_request` that traps (IC0503) is NOT method-absent,
        // even when its message happens to contain a "has no query method"
        // phrase (e.g. bubbled up from a downstream call). Trusting the present
        // error code prevents the false negative this probe exists to avoid.
        let err = reject(
            Some("IC0503"),
            "downstream canister xyz has no query method 'http_request'",
        );
        assert!(!is_method_not_found(&err));
    }

    #[test]
    fn unrelated_missing_method_message_without_error_code_is_ignored() {
        // With no `error_code`, a "has no query method" message about some
        // *other* method must not count as `http_request` being absent.
        let err = reject(None, "Canister abc has no query method 'greet'");
        assert!(!is_method_not_found(&err));
    }

    #[test]
    fn trap_from_wrong_arg_count_is_not_method_not_found() {
        // The method exists but can't decode a zero-argument call, so it traps
        // (IC0503) — the canister still speaks the protocol, so this is a
        // frontend, not a missing method.
        let trap = reject(
            Some("IC0503"),
            "Canister abc trapped explicitly: failed to decode call arguments",
        );
        let contract = reject(Some("IC0504"), "Canister abc violated contract");
        assert!(!is_method_not_found(&trap));
        assert!(!is_method_not_found(&contract));
    }

    #[test]
    fn non_reject_error_is_inconclusive_not_method_not_found() {
        // Transport/other errors are not evidence the method is absent; the
        // false-positive bias then treats the canister as a frontend.
        assert!(!is_method_not_found(&AgentError::InvalidReplicaStatus));
    }
}
