//! Deploy as one operation: build, create, wire, check, install, sync.
//!
//! Deploy used to be orchestration written into the command, with each phase
//! opening its own renderer and the phase headings printed directly between
//! them — the sequencing was what kept those headings from tearing through a
//! live progress view. Here the whole run is a single task tree on a single
//! event stream: each phase is a heading task, and the per-canister tasks the
//! sub-operations start nest under it, because the reporter they are handed
//! is scoped to the phase. They do not know they are being composed.
//!
//! Nothing in here writes to the terminal. Progress goes out as events;
//! results come back on [`DeployReport`] for the command to print. That split
//! is what lets the same orchestration run somewhere without a terminal at
//! all.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Duration;

use candid::Principal;
use futures::{StreamExt, future::try_join_all, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError};
use ic_management_canister_types::{CanisterId, CanisterIdRecord, CanisterInstallMode};
use icp_events::TaskOutcome;
use itertools::Itertools;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::context::{
    CanisterSelection, Context, EnvironmentSelection, GetAgentForEnvError,
    GetCanisterIdForEnvError, GetEnvCanisterError, GetEnvironmentError, GetIdsByEnvironmentError,
    SetCanisterIdForEnvError,
};
use crate::fs::lock::LockError;
use crate::identity::IdentitySelection;
use crate::operations::{
    binding_env_vars::{SetBindingEnvVarsManyError, set_binding_env_vars_many},
    build::{BuildManyError, build_many},
    candid_compat::{CandidCheckManyError, check_candid_compatibility_many},
    create::{CreateFunding, CreateOperation, CreateOperationError, CreateTarget},
    install::{
        InstallManyError, ResolveInstallModeError, install_many, resolve_install_mode_and_status,
    },
    proxy::UpdateOrProxyError,
    proxy_management,
    settings::{
        SyncControllerDependentsError, SyncSettingsManyError, sync_controller_dependents,
        sync_settings_many,
    },
    sync::{SyncOperationError, sync_many},
    task::{Reporter, Task, TaskReporter, notice},
};
use crate::project::ArgsField;
use crate::{CanisterArgsToBytesError, ProjectLoadError};

/// Everything that can stop a deploy. Each phase's failure keeps the typed
/// error of the operation that produced it, so a caller can still tell a
/// build failure from a failed install.
#[derive(Debug, Snafu)]
pub enum DeployError {
    /// Sub-operation failures forward as they are: each already names what it
    /// was doing and which canister it was doing it to, so a deploy-level
    /// wrapper would only say it twice.
    #[snafu(transparent)]
    ResolveBuildTargets { source: GetEnvCanisterError },

    #[snafu(transparent)]
    PackageCache { source: LockError },

    #[snafu(transparent)]
    Build { source: BuildManyError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(transparent)]
    GetAgent { source: GetAgentForEnvError },

    #[snafu(transparent)]
    GetIds { source: GetIdsByEnvironmentError },

    #[snafu(display(
        "`--no-create` was specified but the following canisters do not exist: {}",
        canisters.iter().format(", ")
    ))]
    NoCreate { canisters: Vec<String> },

    #[snafu(transparent)]
    Create { source: CreateOperationError },

    #[snafu(transparent)]
    RecordCanisterId { source: SetCanisterIdForEnvError },

    #[snafu(display(
        "Failed to apply settings that were waiting on canister '{canister}' to exist"
    ))]
    SyncControllerDependents {
        canister: String,
        source: SyncControllerDependentsError,
    },

    #[snafu(transparent)]
    GetCanisterId { source: GetCanisterIdForEnvError },

    /// The id store and the environment disagree: an id is recorded for a
    /// canister the manifest does not declare.
    #[snafu(display("Canister id exists but no canister info: '{canister}'"))]
    MissingCanisterInfo { canister: String },

    #[snafu(display("{message}"))]
    CanisterNotInEnvironment { message: String },

    #[snafu(transparent)]
    SetEnvironmentVariables { source: SetBindingEnvVarsManyError },

    #[snafu(transparent)]
    ApplySettings { source: SyncSettingsManyError },

    #[snafu(transparent)]
    ResolveInstallMode { source: ResolveInstallModeError },

    #[snafu(display("Failed to encode the {field} of canister '{canister}'"))]
    InstallArgs {
        canister: String,
        field: ArgsField,
        source: CanisterArgsToBytesError,
    },

    #[snafu(transparent)]
    CandidCheck { source: CandidCheckManyError },

    #[snafu(transparent)]
    Install { source: InstallManyError },

    #[snafu(display("Failed to start canister {canister_id} before syncing it"))]
    StartCanister {
        canister_id: Principal,
        source: UpdateOrProxyError,
    },

    #[snafu(display(
        "canister {canister_id} did not start serving queries within {seconds}s after being \
         started; the asset sync plugin's first call would fail. Re-run the deploy."
    ))]
    NotServingQueries {
        canister_id: Principal,
        seconds: u64,
    },

    #[snafu(transparent)]
    Sync { source: SyncOperationError },
}

/// Everything that can stop [`resolve_targets`] before a deploy begins.
#[derive(Debug, Snafu)]
pub enum ResolveTargetsError {
    #[snafu(transparent)]
    LoadEnvironment { source: GetEnvironmentError },

    #[snafu(transparent)]
    LoadProject { source: ProjectLoadError },

    #[snafu(transparent)]
    LoadCanisterIds { source: GetIdsByEnvironmentError },

    #[snafu(display(
        "this sub-project depends on canister(s) not yet deployed in the workspace: {}. \
         Run `icp deploy` from the workspace root first (or deploy them explicitly by name).",
        canisters.iter().format(", ")
    ))]
    MissingWorkspaceDependencies { canisters: Vec<String> },
}

/// Everything a deploy needs that the command line supplies. Resolved by the
/// command so this layer never touches clap.
pub struct DeployParams {
    pub environment: EnvironmentSelection,
    pub identity: IdentitySelection,
    /// Canisters to deploy, already resolved from the command line (or from
    /// the environment when none were named).
    pub canisters: Vec<String>,
    pub mode: String,
    pub subnet: Option<Principal>,
    pub proxy: Option<Principal>,
    pub cycles: u128,
    pub no_create: bool,
    /// Skip the Candid interface compatibility check.
    pub yes: bool,
    /// Install arguments, already resolved to bytes. Used whatever the install
    /// mode turns out to be. Only ever set when a single canister is being
    /// deployed.
    pub args: Option<Vec<u8>>,
}

/// What the deploy did, for the command to report.
///
/// Filled in as the run progresses rather than returned at the end: a canister
/// created before a later phase failed still exists, and its id is still what
/// the caller needs to pick the run back up. So the caller owns this and reads
/// it whichever way the deploy went.
#[derive(Debug, Default)]
pub struct DeployReport {
    /// Canisters created during this deploy, in creation order.
    pub created: Vec<(String, Principal)>,
}

/// Run a full deploy, reporting progress as one task tree.
///
/// `report` is written as the run goes; see [`DeployReport`].
pub async fn deploy(
    ctx: &Context,
    params: &DeployParams,
    reporter: &Reporter,
    report: &mut DeployReport,
) -> Result<(), DeployError> {
    let environment_selection = &params.environment;
    let cnames = &params.canisters;

    let canisters_to_build = try_join_all(
        cnames
            .iter()
            .map(|name| ctx.get_canister_and_path_for_env(name, environment_selection)),
    )
    .await?;

    // Build
    let pkg_cache = ctx.dirs.package_cache()?;
    let phase = reporter.task(Task::phase("Building canisters:"));
    let result = build_many(
        canisters_to_build,
        environment_selection.name(),
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &pkg_cache,
        &phase.reporter(),
    )
    .await;
    finish(&phase, result)?;

    // Create any canisters that do not exist yet
    let env = ctx.get_environment(environment_selection).await?;
    let agent = ctx
        .get_agent_for_env(&params.identity, environment_selection)
        .await?;
    let existing_canisters = ctx.ids_by_environment(environment_selection).await?;
    let canisters_to_create = cnames
        .iter()
        .filter(|name| !existing_canisters.contains_key(*name))
        .collect::<Vec<_>>();

    if canisters_to_create.is_empty() {
        notice(reporter, "All canisters already exist");
    } else if params.no_create {
        return NoCreateSnafu {
            canisters: canisters_to_create.into_iter().cloned().collect::<Vec<_>>(),
        }
        .fail();
    } else {
        let phase = reporter.task(Task::phase("Creating canisters:"));
        let result = create_canisters(
            ctx,
            params,
            &agent,
            &env,
            &canisters_to_create,
            existing_canisters.into_values().collect(),
            &phase.reporter(),
            &mut report.created,
        )
        .await;
        finish(&phase, result)?;
    }

    ctx.update_custom_domains(environment_selection).await;

    // Wire canister ids into each other's environment variables, then apply
    // manifest settings.
    let env = ctx.get_environment(environment_selection).await?;
    let env_canisters = &env.canisters;
    let target_canisters = try_join_all(cnames.iter().map(|name| async move {
        let cid = ctx
            .get_canister_id_for_env(
                &CanisterSelection::Named(name.clone()),
                environment_selection,
            )
            .await?;
        let (_, info) = env_canisters
            .get(name)
            .context(MissingCanisterInfoSnafu { canister: name })?;
        Ok::<_, DeployError>((cid, info.clone()))
    }))
    .await?;

    let canister_list = ctx.ids_by_environment(environment_selection).await?;

    let phase = reporter.task(Task::phase("Setting environment variables:"));
    let result = set_binding_env_vars_many(
        agent.clone(),
        params.proxy,
        &env.name,
        target_canisters.clone(),
        canister_list.clone(),
        &phase.reporter(),
    )
    .await;
    finish(&phase, result)?;

    let phase = reporter.task(Task::phase("Applying canister settings:"));
    let result = sync_settings_many(
        agent.clone(),
        params.proxy,
        target_canisters,
        canister_list,
        &env,
        &phase.reporter(),
    )
    .await;
    finish(&phase, result)?;

    // Resolve install plans
    let canisters = try_join_all(cnames.iter().map(|name| {
        let agent = agent.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(
                    &CanisterSelection::Named(name.clone()),
                    environment_selection,
                )
                .await?;

            let (mode, status) =
                resolve_install_mode_and_status(&agent, params.proxy, name, &cid, &params.mode)
                    .await?;

            let env = ctx.get_environment(environment_selection).await?;
            let (_canister_path, canister_info) = env
                .get_canister_info(name)
                .map_err(|message| DeployError::CanisterNotInEnvironment { message })?;

            // An upgrade passes `upgrade_args`; a canister that declares none is
            // upgraded with its `init_args`, which its post-upgrade entry point
            // expects anyway. The field is carried along so a malformed value is
            // reported against the one the user wrote.
            let init_args = || {
                canister_info
                    .init_args
                    .as_ref()
                    .map(|a| (ArgsField::Init, a))
            };
            let manifest_args = match mode {
                CanisterInstallMode::Upgrade(_) => canister_info
                    .upgrade_args
                    .as_ref()
                    .map(|a| (ArgsField::Upgrade, a))
                    .or_else(init_args),
                CanisterInstallMode::Install | CanisterInstallMode::Reinstall => init_args(),
            };

            // Command-line arguments take priority over the manifest's.
            let args_bytes = match (&params.args, manifest_args) {
                (Some(bytes), _) => Some(bytes.clone()),
                (None, Some((field, a))) => Some(a.to_bytes().context(InstallArgsSnafu {
                    canister: name,
                    field,
                })?),
                (None, None) => None,
            };

            Ok::<_, DeployError>((name.clone(), cid, mode, status, args_bytes))
        }
    }))
    .await?;

    if !params.yes {
        let phase = reporter.task(Task::phase("Checking compatibility:"));
        let result = check_candid_compatibility_many(
            agent.clone(),
            canisters
                .iter()
                .map(|(name, cid, mode, _, _)| (&**name, *cid, *mode)),
            ctx.artifacts.clone(),
            &phase.reporter(),
        )
        .await;
        finish(&phase, result)?;
    }

    // Install
    let phase = reporter.task(Task::phase("Installing canisters:"));
    let result = install_many(
        agent.clone(),
        params.proxy,
        canisters,
        ctx.artifacts.clone(),
        &phase.reporter(),
    )
    .await;
    finish(&phase, result)?;

    sync(ctx, params, &agent, reporter).await?;

    Ok(())
}

/// Create the missing canisters, recording each id as it lands.
///
/// Ids are appended to `created` as each canister comes into existence, before
/// anything that could still fail, so a partial run still reports what it made.
#[allow(clippy::too_many_arguments)]
async fn create_canisters(
    ctx: &Context,
    params: &DeployParams,
    agent: &Agent,
    env: &crate::Environment,
    canisters_to_create: &[&String],
    existing_ids: Vec<Principal>,
    reporter: &Reporter,
    created: &mut Vec<(String, Principal)>,
) -> Result<(), DeployError> {
    let target = match (params.subnet, params.proxy) {
        (Some(subnet), _) => CreateTarget::Subnet(subnet),
        (_, Some(proxy)) => CreateTarget::Proxy(proxy),
        _ => CreateTarget::None,
    };
    let create_operation = CreateOperation::new(
        agent.clone(),
        target,
        CreateFunding::Cycles(params.cycles),
        existing_ids,
    );

    let mut futs = FuturesOrdered::new();
    for name in canisters_to_create.iter() {
        let task = reporter.task(Task::create((*name).clone()));
        let create_op = create_operation.clone();
        let (_, canister_info) = env
            .get_canister_info(name)
            .map_err(|message| DeployError::CanisterNotInEnvironment { message })?;
        futs.push_back(async move {
            let result = create_op.create(&canister_info.settings.into()).await;

            match &result {
                Ok(_) => task.finish(TaskOutcome::succeeded()),
                Err(err) => task.finish(TaskOutcome::failed(err.to_string())),
            }

            result
        });
    }

    // Every creation result must be drained before returning. A create call still
    // in flight when we bail may already have made its canister on the IC, so
    // abandoning the remaining futures would lose that id for good — which is the
    // very loss this loop exists to prevent. So no step below short-circuits the
    // loop: the first error is held back and returned once the stream is empty.
    let mut error: Option<DeployError> = None;
    let mut idx = 0;
    while let Some(res) = futs.next().await {
        match res {
            Ok(id) => {
                let canister_name = canisters_to_create
                    .get(idx)
                    .expect("should have tried to create every canister");
                // Report the id before recording it or wiring up dependents: the
                // canister exists from here on, and if either of those fails this
                // is the only place the user will see the id it was given.
                created.push(((*canister_name).clone(), id));

                // Scoped to this canister rather than the loop, so a failure here
                // holds up only its own follow-up work.
                let result = async {
                    ctx.set_canister_id_for_env(canister_name, id, &params.environment)
                        .await?;
                    // Apply controller settings for any already-created canister that
                    // was waiting for this one to exist (e.g. created via
                    // `icp canister create`). Skipped when the id never reached the
                    // store, since that is what a dependent would be looking it up in.
                    sync_controller_dependents(
                        ctx,
                        agent,
                        params.proxy,
                        canister_name,
                        &params.environment,
                    )
                    .await
                    .context(SyncControllerDependentsSnafu {
                        canister: (*canister_name).clone(),
                    })
                }
                .await;

                if let Err(err) = result {
                    error.get_or_insert(err);
                }
            }
            Err(err) => {
                error.get_or_insert(err.into());
            }
        }
        idx += 1;
    }
    match error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Run the sync steps of every canister that has any.
async fn sync(
    ctx: &Context,
    params: &DeployParams,
    agent: &Agent,
    reporter: &Reporter,
) -> Result<(), DeployError> {
    let environment_selection = &params.environment;
    let env = ctx.get_environment(environment_selection).await?;

    let env_canisters = &env.canisters;
    let sync_canisters = try_join_all(params.canisters.iter().map(|name| async move {
        let cid = ctx
            .get_canister_id_for_env(
                &CanisterSelection::Named(name.clone()),
                environment_selection,
            )
            .await?;
        let (canister_path, info) = env_canisters
            .get(name)
            .context(MissingCanisterInfoSnafu { canister: name })?;
        Ok::<_, DeployError>((cid, canister_path.clone(), info.clone()))
    }))
    .await?;

    // Filter out canisters with no sync steps
    let sync_canisters: Vec<_> = sync_canisters
        .into_iter()
        .filter(|(_, _, info)| !info.sync.steps.is_empty())
        .collect();

    if sync_canisters.is_empty() {
        notice(reporter, "No canisters have sync steps configured");
        return Ok(());
    }

    // Asset sync requires the canister to be Running. install_code is status-
    // preserving, so a canister that entered deploy Stopped/Stopping (handed out
    // Stopped from a pool, or left so by an earlier interrupted deploy) is still
    // not Running here. Start each canister we're about to sync. Per the IC spec
    // start_canister is synchronous — its Ok reply means the canister is already
    // Running, so no status poll is needed — and idempotent (no-op if Running).
    let proxy = params.proxy;
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
            .context(StartCanisterSnafu { canister_id: cid })
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
    let canister_ids: BTreeMap<String, Principal> = ctx
        .ids_by_environment(environment_selection)
        .await?
        .into_iter()
        .collect();

    let pkg_cache = ctx.dirs.package_cache()?;

    let phase = reporter.task(Task::phase("Syncing canisters:"));
    let result = sync_many(
        ctx.syncer.clone(),
        agent.clone(),
        sync_canisters,
        environment_selection.name().to_owned(),
        env.network.name.clone(),
        canister_ids,
        proxy,
        &pkg_cache,
        &phase.reporter(),
    )
    .await;
    finish(&phase, result)?;

    Ok(())
}

/// Close a phase's heading task from its result, and hand the result back.
///
/// The heading carries no failure text of its own: whichever child failed
/// already said what went wrong, and the error itself is on the return path.
fn finish<T, E: std::fmt::Display>(phase: &TaskReporter, result: Result<T, E>) -> Result<T, E> {
    match &result {
        Ok(_) => phase.finish(TaskOutcome::succeeded()),
        Err(error) => phase.finish(TaskOutcome::failed(error.to_string())),
    }
    result
}

/// Resolve the canisters a deploy targets, and check that a member-scoped
/// deploy is not about to wire canisters to dependencies that do not exist.
pub async fn resolve_targets(
    ctx: &Context,
    environment_selection: &EnvironmentSelection,
    named: &[String],
) -> Result<Vec<String>, ResolveTargetsError> {
    let env = ctx.get_environment(environment_selection).await?;

    let mut member_scoped = false;
    let cnames: Vec<String> = if named.is_empty() {
        // No canisters specified: default to the whole environment, unless the
        // command is run inside a vendored member — then scope to that member's
        // own canisters. (The resolved-root notice is emitted centrally during
        // project load.)
        let project = ctx.project.load().await?;
        let member_dir = ctx.project.member_dir();
        match crate::project::member_scoped_canisters(&project.dir, member_dir.as_deref(), &env) {
            Some(scoped) => {
                member_scoped = true;
                scoped
            }
            None => env.canisters.keys().cloned().collect(),
        }
    } else {
        named.to_vec()
    };

    // A member-scoped deploy targets only the sub-project's own canisters, but
    // those canisters are wired to their dependencies' ids — and the dependency
    // canisters are outside the scope, so they are not (re)deployed here. If any
    // are missing from the workspace store, fail fast rather than silently
    // deploying an unwired canister.
    if member_scoped {
        let scoped: HashSet<&str> = cnames.iter().map(String::as_str).collect();
        let deployed: BTreeMap<String, Principal> = ctx
            .ids_by_environment(environment_selection)
            .await?
            .into_iter()
            .collect();
        let mut missing: BTreeSet<String> = BTreeSet::new();
        for name in &cnames {
            if let Some((_, canister)) = env.canisters.get(name) {
                for target in canister.bindings.values() {
                    // A target the environment does not contain at all was
                    // deliberately left out of it, so no deploy from anywhere in
                    // the workspace can give it an id here. Waiting on it would
                    // block this sub-project forever.
                    if !env.canisters.contains_key(target) {
                        continue;
                    }
                    if !scoped.contains(target.as_str()) && !deployed.contains_key(target) {
                        missing.insert(target.clone());
                    }
                }
            }
        }
        if !missing.is_empty() {
            return MissingWorkspaceDependenciesSnafu {
                canisters: missing.into_iter().collect::<Vec<_>>(),
            }
            .fail();
        }
    }

    Ok(cnames)
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
) -> Result<(), DeployError> {
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
        Err(_elapsed) => NotServingQueriesSnafu {
            canister_id,
            seconds: READINESS_BUDGET.as_secs(),
        }
        .fail(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ic_agent::agent::{RejectCode, RejectResponse};

    fn reject(error_code: Option<&str>, reject_message: &str) -> AgentError {
        AgentError::UncertifiedReject {
            reject: RejectResponse {
                reject_code: RejectCode::CanisterError,
                reject_message: reject_message.to_string(),
                error_code: error_code.map(String::from),
            },
            operation: None,
        }
    }

    #[test]
    fn stopped_rejects_are_not_a_readiness_signal() {
        // A replica still lagging behind the restart.
        assert!(!is_serving_reject(&reject(
            Some("IC0508"),
            "Canister abc is stopped"
        )));
        assert!(!is_serving_reject(&reject(
            None,
            "Canister abc is stopping"
        )));
    }

    #[test]
    fn any_other_reject_means_the_canister_is_serving() {
        // The replica got far enough to answer for a non-status reason.
        assert!(is_serving_reject(&reject(
            Some("IC0536"),
            "Canister abc has no query method '<icp-cli readiness probe>'"
        )));
    }

    #[test]
    fn a_transport_error_is_inconclusive() {
        // Not evidence of anything; the caller must retry.
        assert!(!is_serving_reject(&AgentError::InvalidReplicaStatus));
    }
}
