use anyhow::{anyhow, bail};
use candid::{CandidType, Principal};
use clap::Args;
use futures::{StreamExt, future::try_join_all, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError};
use ic_management_canister_types::{CanisterId, CanisterIdRecord};
use icp::parsers::CyclesAmount;
use icp::{
    context::{CanisterSelection, Context, EnvironmentSelection},
    identity::IdentitySelection,
    network::Configuration as NetworkConfiguration,
};
use icp_canister_interfaces::candid_ui::MAINNET_CANDID_UI_CID;
use itertools::Itertools;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Duration;
use tracing::info;

use crate::{
    commands::{args::ArgsOpt, canister::create},
    operations::{
        binding_env_vars::set_binding_env_vars_many,
        build::build_many_with_progress_bar,
        candid_compat::check_candid_compatibility_many,
        create::{CreateFunding, CreateOperation, CreateTarget},
        install::{install_many, resolve_install_mode_and_status},
        proxy_management,
        settings::{sync_controller_dependents, sync_settings_many},
        sync::sync_many,
    },
    options::{EnvironmentOpt, IdentityOpt},
    progress::{ProgressManager, ProgressManagerSettings},
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
    pub(crate) environment: EnvironmentOpt,

    /// Output command results as JSON
    #[arg(long)]
    pub(crate) json: bool,

    /// Arguments to pass to the canister on install.
    /// Only valid when deploying a single canister. Takes priority over `init_args` in the manifest.
    #[command(flatten)]
    pub(crate) args_opt: ArgsOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &DeployArgs) -> Result<(), anyhow::Error> {
    let environment_selection: EnvironmentSelection = args.environment.clone().into();
    let identity_selection: IdentitySelection = args.identity.clone().into();

    let env = ctx.get_environment(&environment_selection).await?;

    let mut member_scoped = false;
    let cnames: Vec<String> = if args.names.is_empty() {
        // No canisters specified: default to the whole environment, unless the
        // command is run inside a vendored member — then scope to that member's
        // own canisters. (The resolved-root notice is emitted centrally during
        // project load.)
        let project = ctx.project.load().await?;
        let member_dir = ctx.project.member_dir();
        match icp::project::member_scoped_canisters(&project.dir, member_dir.as_deref(), &env) {
            Some(scoped) => {
                member_scoped = true;
                scoped
            }
            None => env.canisters.keys().cloned().collect(),
        }
    } else {
        // Individual canisters specified.
        args.names.clone()
    };

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    if args.args_opt.is_some() && cnames.len() != 1 {
        anyhow::bail!("--args and --args-file can only be used when deploying a single canister");
    }

    // A member-scoped deploy targets only the sub-project's own canisters, but
    // those canisters are wired to their dependencies' ids — and the dependency
    // canisters are outside the scope, so they are not (re)deployed here. If any
    // are missing from the workspace store, fail fast rather than silently
    // deploying an unwired canister.
    if member_scoped {
        let scoped: HashSet<&str> = cnames.iter().map(String::as_str).collect();
        let deployed: BTreeMap<String, Principal> = ctx
            .ids_by_environment(&environment_selection)
            .await?
            .into_iter()
            .collect();
        let mut missing: BTreeSet<String> = BTreeSet::new();
        for name in &cnames {
            if let Some((_, canister)) = env.canisters.get(name) {
                for target in canister.bindings.values() {
                    if !scoped.contains(target.as_str()) && !deployed.contains_key(target) {
                        missing.insert(target.clone());
                    }
                }
            }
        }
        if !missing.is_empty() {
            anyhow::bail!(
                "this sub-project depends on canister(s) not yet deployed in the workspace: {}. \
                 Run `icp deploy` from the workspace root first (or deploy them explicitly by name).",
                missing.into_iter().collect::<Vec<_>>().join(", ")
            );
        }
    }

    let canisters_to_build = try_join_all(
        cnames
            .iter()
            .map(|name| ctx.get_canister_and_path_for_env(name, &environment_selection)),
    )
    .await?;

    // Build the selected canisters
    info!("Building canisters:");

    build_many_with_progress_bar(
        canisters_to_build,
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.dirs.package_cache()?,
        ctx.debug,
    )
    .await?;

    // Ensure the selected canisters exist, creating any that are missing.
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let agent = ctx
        .get_agent_for_env(&identity_selection, &environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let existing_canisters = ctx
        .ids_by_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;
    let canisters_to_create = cnames
        .iter()
        .filter(|name| !existing_canisters.contains_key(*name))
        .collect::<Vec<_>>();

    if canisters_to_create.is_empty() {
        info!("All canisters already exist");
    } else if args.no_create {
        bail!(
            "`--no-create` was specified but the following canisters do not exist: {}",
            canisters_to_create.iter().format(", ")
        );
    } else {
        info!("Creating canisters:");
        let target = match (args.subnet, args.proxy) {
            (Some(subnet), _) => CreateTarget::Subnet(subnet),
            (_, Some(proxy)) => CreateTarget::Proxy(proxy),
            _ => CreateTarget::None,
        };
        let create_operation = CreateOperation::new(
            agent.clone(),
            target,
            CreateFunding::Cycles(args.cycles.get()),
            existing_canisters.into_values().collect(),
        );
        let mut futs = FuturesOrdered::new();
        let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });
        for name in canisters_to_create.iter() {
            let pb = progress_manager.create_progress_bar(name);
            pb.set_message("Creating...");
            let create_op = create_operation.clone();
            let (_, canister_info) = env.get_canister_info(name).map_err(|e| anyhow!(e))?;
            futs.push_back(async move {
                ProgressManager::execute_with_custom_progress(
                    &pb,
                    create_op.create(&canister_info.settings.into()),
                    || "Created successfully".to_string(),
                    |err: &_| err.to_string(),
                    |_| false,
                )
                .await
            });
        }

        // Cache errors until all futures are processed. Otherwise we risk dropping a canister id.
        let mut error: Option<anyhow::Error> = None;
        let mut idx = 0;
        while let Some(res) = futs.next().await {
            match res {
                Ok(id) => {
                    let canister_name = canisters_to_create
                        .get(idx)
                        .expect("should have tried to create every canister");
                    if !args.json {
                        println!("Created canister {canister_name} with ID {id}");
                    }
                    ctx.set_canister_id_for_env(canister_name, id, &environment_selection)
                        .await
                        .map_err(|e| anyhow!(e))?;
                    // Apply controller settings for any already-created canister that was
                    // waiting for this one to exist (e.g. created via `icp canister create`).
                    sync_controller_dependents(
                        ctx,
                        &agent,
                        args.proxy,
                        canister_name,
                        &environment_selection,
                    )
                    .await
                    .map_err(|e| anyhow!(e))?;
                }
                Err(err) => {
                    error = Some(err.into());
                }
            }
            idx += 1;
        }
        if let Some(err) = error {
            return Err(err);
        }
    }

    ctx.update_custom_domains(&environment_selection).await;

    info!("Setting environment variables:");
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    let env_canisters = &env.canisters;
    let target_canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    &environment_selection,
                )
                .await
                .map_err(|e| anyhow!(e))?;
            let (_, info) = env_canisters
                .get(name)
                .ok_or_else(|| anyhow!("Canister id exists but no canister info"))?;
            Ok::<_, anyhow::Error>((cid, info.clone()))
        }
    }))
    .await?;

    let canister_list = ctx
        .ids_by_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    set_binding_env_vars_many(
        agent.clone(),
        args.proxy,
        &env.name,
        target_canisters.clone(),
        canister_list.clone(),
        ctx.debug,
    )
    .await
    .map_err(|e| anyhow!(e))?;

    sync_settings_many(
        agent.clone(),
        args.proxy,
        target_canisters,
        canister_list,
        ctx.debug,
    )
    .await
    .map_err(|e| anyhow!(e))?;

    // Install the selected canisters

    let canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        let agent = agent.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    &environment_selection,
                )
                .await
                .map_err(|e| anyhow!(e))?;

            let (mode, status) =
                resolve_install_mode_and_status(&agent, args.proxy, name, &cid, &args.mode).await?;

            let env = ctx.get_environment(&environment_selection).await?;
            let (_canister_path, canister_info) =
                env.get_canister_info(name).map_err(|e| anyhow!(e))?;

            // CLI --args/--args-file take priority over manifest init_args
            let init_args_bytes = if args.args_opt.is_some() {
                args.args_opt.resolve_bytes()?
            } else {
                canister_info
                    .init_args
                    .as_ref()
                    .map(|ia| ia.to_bytes())
                    .transpose()?
            };

            Ok::<_, anyhow::Error>((name.clone(), cid, mode, status, init_args_bytes))
        }
    }))
    .await?;

    if !args.yes {
        info!("Checking compatibility:");
        check_candid_compatibility_many(
            agent.clone(),
            canisters
                .iter()
                .map(|(name, cid, mode, _, _)| (&**name, *cid, *mode)),
            ctx.artifacts.clone(),
            ctx.debug,
        )
        .await
        .map_err(|e| anyhow!(e))?;
    }

    info!("Installing canisters:");

    install_many(
        agent.clone(),
        args.proxy,
        canisters,
        ctx.artifacts.clone(),
        ctx.debug,
    )
    .await?;

    // Sync the selected canisters

    // Prepare list of canisters with their info for syncing
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    let env_canisters = &env.canisters;
    let sync_canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    &environment_selection,
                )
                .await
                .map_err(|e| anyhow!(e))?;
            let (canister_path, info) = env_canisters
                .get(name)
                .ok_or_else(|| anyhow!("Canister id exists but no canister info"))?;
            Ok::<_, anyhow::Error>((cid, canister_path.clone(), info.clone()))
        }
    }))
    .await?;

    // Filter out canisters with no sync steps
    let sync_canisters: Vec<_> = sync_canisters
        .into_iter()
        .filter(|(_, _, info)| !info.sync.steps.is_empty())
        .collect();

    if sync_canisters.is_empty() {
        info!("No canisters have sync steps configured");
    } else {
        // Asset sync requires the canister to be Running. install_code is status-
        // preserving, so a canister that entered deploy Stopped/Stopping (handed out
        // Stopped from a pool, or left so by an earlier interrupted deploy) is still
        // not Running here. Start each canister we're about to sync. Per the IC spec
        // start_canister is synchronous — its Ok reply means the canister is already
        // Running, so no status poll is needed — and idempotent (no-op if Running).
        let proxy = args.proxy;
        try_join_all(sync_canisters.iter().map(|(cid, _, _)| {
            let agent = agent.clone();
            let cid = *cid;
            async move {
                proxy_management::start_canister(
                    &agent,
                    proxy,
                    CanisterIdRecord {
                        canister_id: CanisterId::from(cid),
                    },
                )
                .await
                .map_err(|e| anyhow!(e))
            }
        }))
        .await?;

        // start_canister is synchronous, so each canister is now Running in the
        // subnet's *certified* state — but IC query calls are eventually-consistent
        // reads, answered by a single replica that may still lag the height at which
        // the restart committed and would then observe the just-vacated Stopped state.
        // The sync plugin's first calls are queries, so without this wait sync can fail
        // with a transient IC0508 right after a restart. Wait until the query path
        // consistently sees the canister Running before handing off.
        try_join_all(sync_canisters.iter().map(|(cid, _, _)| {
            let agent = agent.clone();
            let cid = *cid;
            async move { wait_until_serving_queries(&agent, cid).await }
        }))
        .await?;

        // TODO: When `--proxy` is used and the canister was newly created, the proxy
        // canister is its only controller. Sync steps (e.g. asset uploads to a frontend
        // canister) will fail because the user's identity lacks the required permissions.
        // The fix is to make a proxy call to the frontend canister's `grant_permission`
        // method to permit the user identity to upload assets directly before syncing.
        info!("Syncing canisters:");

        let canister_ids: BTreeMap<String, Principal> = ctx
            .ids_by_environment(&environment_selection)
            .await?
            .into_iter()
            .collect();

        let pkg_cache = ctx.dirs.package_cache()?;
        sync_many(
            ctx.syncer.clone(),
            agent.clone(),
            sync_canisters,
            environment_selection.name().to_owned(),
            env.network.name.clone(),
            canister_ids,
            args.proxy,
            ctx.debug,
            &pkg_cache,
        )
        .await?;
    }

    // Print URLs for deployed canisters
    print_canister_urls(
        ctx,
        &environment_selection,
        agent.clone(),
        &cnames,
        args.json,
    )
    .await?;

    Ok(())
}

/// A method name no real canister exports — used purely as a liveness probe.
/// Querying it is side-effect-free: the replica rejects an unknown method before
/// any canister code runs (no cycles, no logs, no state change), and the reject
/// reason tells us whether the canister is serving queries yet.
const READINESS_PROBE_METHOD: &str = "<icp-cli readiness probe>";

/// Wait until the canister's *query* path consistently observes it as Running.
///
/// After `start_canister` the canister is Running in the subnet's certified
/// state, but query calls are eventually-consistent reads: each is answered by a
/// single replica that may still lag the restart's commit height and would then
/// see the just-vacated Stopped state. The sync plugin's first calls are queries,
/// so without this wait sync can fail with a transient IC0508 right after a
/// restart.
///
/// We probe with a query for a method no canister exports and classify the result:
///
/// - a reject of "is stopped"/"is stopping" (IC0508/IC0509) means the replica is
///   still lagging behind the restart.
/// - any other reject (e.g. "no query method"), or a reply, means the replica got
///   far enough to answer for a non-status reason, so it sees the canister Running.
/// - a transport or timeout error is inconclusive.
///
/// We require a few consecutive ready observations, spaced out so they may land on
/// different replicas, to raise confidence the lagging set has drained. This is not
/// a hard guarantee — query reads are per-node and boundary nodes load-balance
/// across replicas — but it makes the post-restart race rare.
async fn wait_until_serving_queries(
    agent: &Agent,
    canister_id: Principal,
) -> Result<(), anyhow::Error> {
    const REQUIRED_CONSECUTIVE: u32 = 2;
    // Total wall-clock budget for the whole wait — the hard cap on the failure
    // path. PROBE_TIMEOUT below only bounds a single hung probe (so retries keep
    // flowing); this outer budget is what guarantees we give up promptly, rather
    // than attempts * (probe timeout + interval).
    const READINESS_BUDGET: Duration = Duration::from_secs(30);
    const POLL_INTERVAL: Duration = Duration::from_millis(500);
    const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

    let poll = async {
        let mut consecutive_ready: u32 = 0;
        loop {
            let probe = agent
                .query(&canister_id, READINESS_PROBE_METHOD)
                .with_arg(Vec::<u8>::new())
                .call();
            let ready = match tokio::time::timeout(PROBE_TIMEOUT, probe).await {
                Ok(Ok(_)) => true,                       // replied -> Running
                Ok(Err(err)) => is_serving_reject(&err), // non-stopped reject -> Running
                Err(_elapsed) => false,                  // probe timed out -> inconclusive
            };

            if ready {
                consecutive_ready += 1;
                if consecutive_ready >= REQUIRED_CONSECUTIVE {
                    return;
                }
            } else {
                consecutive_ready = 0;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    };

    match tokio::time::timeout(READINESS_BUDGET, poll).await {
        Ok(()) => Ok(()),
        Err(_elapsed) => bail!(
            "canister {canister_id} did not start serving queries within {}s after being \
             started; the asset sync plugin's first call would fail. Re-run the deploy.",
            READINESS_BUDGET.as_secs()
        ),
    }
}

/// True when a query error is a *reject from the replica* that indicates the
/// canister is Running and serving — i.e. a positive readiness signal.
///
/// A reject means the replica processed the request to a verdict (e.g. "no such
/// query method"), so the canister is up — unless the reject says it is
/// stopped/stopping (IC0508/IC0509, with a message-substring fallback), which is
/// a replica still lagging behind the restart. Every other `AgentError`
/// (transport, HTTP, timeout, …) is inconclusive — not evidence the canister is
/// serving — and returns false so the caller retries rather than proceeding.
fn is_serving_reject(err: &AgentError) -> bool {
    let reject = match err {
        AgentError::CertifiedReject { reject, .. }
        | AgentError::UncertifiedReject { reject, .. } => reject,
        _ => return false,
    };
    let stopped = matches!(
        reject.error_code.as_deref(),
        Some("IC0508") | Some("IC0509")
    ) || reject.reject_message.contains("is stopped")
        || reject.reject_message.contains("is stopping");
    !stopped
}

/// Checks if a canister has an `http_request` function by querying it
async fn has_http_request(agent: &Agent, canister_id: Principal) -> bool {
    #[derive(CandidType, Serialize)]
    struct HttpRequest {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    // Construct an HttpRequest for '/index.html'
    let request = HttpRequest {
        method: "GET".to_string(),
        url: "/index.html".to_string(),
        headers: vec![],
        body: vec![],
    };

    let args = candid::encode_one(&request).expect("failed to encode request");

    // Try to query the http_request endpoint
    let result = agent
        .query(&canister_id, "http_request")
        .with_arg(args)
        .call()
        .await;

    // If the query succeeds (regardless of the response), the canister has http_request
    result.is_ok()
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

    if !json {
        println!("Deployed canisters:");
    }

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

        if let Some(http_gateway_url) = &http_gateway_url {
            let has_http = has_http_request(&agent, canister_id).await;

            if has_http {
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
                    if json {
                        json_canisters.push(JsonDeployedCanister {
                            name: name.clone(),
                            canister_id,
                            url: Some(canister_url.to_string()),
                        });
                    } else {
                        println!("  {name}: {canister_url}");
                    }
                }
            } else {
                // For canisters without http_request, show the Candid UI URL
                let url = if let Some(ui_id) = get_candid_ui_id(ctx, environment_selection).await {
                    let domain = gateway_domain(http_gateway_url);
                    let mut candid_url = canister_gateway_url(http_gateway_url, ui_id, None);
                    if domain.is_some() {
                        candid_url.set_query(Some(&format!("id={canister_id}")));
                    } else {
                        candid_url.set_query(Some(&format!("canisterId={ui_id}&id={canister_id}")));
                    }
                    if !json {
                        println!("  {name} (Candid UI): {candid_url}");
                    }
                    Some(candid_url.to_string())
                } else {
                    if !json {
                        println!("  {name}: {canister_id} (Candid UI not available)");
                    }
                    None
                };
                if json {
                    json_canisters.push(JsonDeployedCanister {
                        name: name.clone(),
                        canister_id,
                        url,
                    });
                }
            }
        } else if json {
            json_canisters.push(JsonDeployedCanister {
                name: name.clone(),
                canister_id,
                url: None,
            });
        } else {
            println!("  {name}: {canister_id} (No gateway URL available)");
        }
    }

    if json {
        serde_json::to_writer(
            std::io::stdout(),
            &JsonDeploy {
                canisters: json_canisters,
            },
        )?;
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
