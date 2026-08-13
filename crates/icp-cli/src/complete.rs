//! Candidates for shell completion.
//!
//! Names the user types are not knowable from the clap definition: canisters,
//! networks and environments come from the project the shell is standing in,
//! identities from the user's identity store. The shell therefore re-invokes
//! `icp` while completing, [`env`] answers that invocation, and the functions
//! below supply the values. `icp completions <SHELL>` (see
//! [`crate::commands::completions`]) emits the script that wires this up.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use clap::CommandFactory as _;
use clap_complete::CompleteEnv;
use clap_complete::engine::CompletionCandidate;
use icp::network::Configuration;
use icp::prelude::*;
use icp::{Environment, Network, Project};

use crate::context::Context;
use crate::identity::manifest::IdentityList;

/// Answer a completion request and exit, if this invocation is one.
///
/// Must run before anything writes to stdout: the completion protocol is
/// carried over it.
pub(crate) fn env() {
    CompleteEnv::with_factory(crate::Cli::command).complete();
}

/// Upper bound on the work one completion request may do. Loading the project
/// and reading the identity store both take file locks that a concurrently
/// running `icp` can hold, and an uncached recipe is fetched over the network;
/// a shell that stops responding is worse than one that offers nothing.
const BUDGET: Duration = Duration::from_secs(2);

/// Run `fut` on a throwaway runtime, giving up after [`BUDGET`].
///
/// Completion is answered from `main` before the application's runtime is
/// started, so there is no ambient one to borrow.
fn block_on<T>(fut: impl Future<Output = T>) -> Option<T> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let value = rt.block_on(async { tokio::time::timeout(BUDGET, fut).await.ok() });
    // Lock acquisition happens on the blocking pool and cannot be cancelled, so
    // dropping the runtime would wait for it and reintroduce the hang the
    // timeout exists to prevent. The process is about to exit regardless.
    rt.shutdown_background();
    value
}

/// The context the candidate functions read through, built once per request.
///
/// `--project-root-override` cannot be honoured here — clap hands completers no
/// parsed arguments — so project resolution follows `ICP_PROJECT_ROOT` and the
/// working directory only.
fn context() -> Option<&'static Context> {
    static CONTEXT: OnceLock<Option<Context>> = OnceLock::new();

    CONTEXT
        .get_or_init(|| {
            crate::context::initialize(
                std::env::var("ICP_PROJECT_ROOT").ok().map(PathBuf::from),
                false,
                Arc::new(|| Err("cannot prompt while completing".to_string())),
                None,
            )
            .ok()
        })
        .as_ref()
}

/// The project the shell is standing in, or `None` outside one.
fn project() -> Option<&'static Project> {
    static PROJECT: OnceLock<Option<Project>> = OnceLock::new();

    PROJECT
        .get_or_init(|| block_on(context()?.project.load())?.ok())
        .as_ref()
}

/// Every identity in the store, paired with the principal to describe it by.
fn identities() -> &'static [(String, String)] {
    static IDENTITIES: OnceLock<Vec<(String, String)>> = OnceLock::new();

    IDENTITIES.get_or_init(|| {
        let Some(dirs) = context().and_then(|ctx| ctx.identity_dirs().ok()) else {
            return Vec::new();
        };
        let Some(Ok(Ok(list))) =
            block_on(dirs.with_read(async |dirs| IdentityList::load_from(dirs)))
        else {
            return Vec::new();
        };

        let mut identities = list
            .identities
            .into_iter()
            .map(|(name, spec)| {
                let description = spec
                    .principal()
                    .map_or_else(|| "pending delegation".to_string(), |p| p.to_string());
                (name, description)
            })
            .collect::<Vec<_>>();
        identities.sort();
        identities
    })
}

/// Canisters declared by the project, in manifest order.
pub(crate) fn canisters() -> Vec<CompletionCandidate> {
    project()
        .into_iter()
        .flat_map(|project| project.canisters.keys())
        .map(|name| CompletionCandidate::new(name.as_str()))
        .collect()
}

/// Networks declared by the project, described by where they point.
pub(crate) fn networks() -> Vec<CompletionCandidate> {
    sorted_by_name(project().map(|project| &project.networks))
        .map(|(name, network)| {
            CompletionCandidate::new(name.as_str()).help(Some(describe_network(network).into()))
        })
        .collect()
}

/// Environments declared by the project, described by the network they use.
pub(crate) fn environments() -> Vec<CompletionCandidate> {
    sorted_by_name(project().map(|project| &project.environments))
        .map(|(name, environment): (_, &Environment)| {
            CompletionCandidate::new(name.as_str()).help(Some(
                format!("network: {}", environment.network.name).into(),
            ))
        })
        .collect()
}

/// Identities in the user's identity store, described by their principal.
pub(crate) fn identity_names() -> Vec<CompletionCandidate> {
    identities()
        .iter()
        .map(|(name, description)| {
            CompletionCandidate::new(name.as_str()).help(Some(description.as_str().into()))
        })
        .collect()
}

fn describe_network(network: &Network) -> String {
    match &network.configuration {
        Configuration::Managed { .. } => "managed locally".to_string(),
        Configuration::Connected { connected } => connected.api_url.to_string(),
    }
}

/// Project maps are unordered; completion output should not be.
fn sorted_by_name<V>(
    map: Option<&HashMap<String, V>>,
) -> impl Iterator<Item = (&String, &V)> + use<'_, V> {
    let mut entries = map.into_iter().flatten().collect::<Vec<_>>();
    entries.sort_by_key(|(name, _)| *name);
    entries.into_iter()
}
