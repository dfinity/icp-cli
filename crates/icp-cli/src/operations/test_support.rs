//! Fixtures shared by the operation unit tests.
//!
//! Operations report through an [`icp_events::Reporter`], so a test can run one for
//! real and assert on the `Vec<Event>` it produced — no terminal, and no need to
//! inspect what a progress bar happened to draw.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use ic_agent::Agent;
use icp::Canister;
use icp::canister::Settings;
use icp::manifest::{BuildSteps, SyncSteps};
use icp::store_artifact::{Access, LookupArtifactError, SaveError};
use icp_events::{Event, Outcome, RecordingSink, Reporter, TaskId};

/// A reporter that records everything instead of drawing it.
pub(crate) fn recording_reporter() -> (Reporter, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::new());
    (Reporter::new(sink.clone()), sink)
}

/// An artifact store that holds nothing, so every lookup misses.
///
/// Lets the install path be exercised without a network: the missing artifact is
/// found before the agent is ever touched.
#[derive(Debug, Default)]
pub(crate) struct EmptyArtifacts;

#[async_trait]
impl Access for EmptyArtifacts {
    async fn save(&self, _name: &str, _wasm: &[u8]) -> Result<(), SaveError> {
        Ok(())
    }

    async fn lookup(&self, name: &str) -> Result<Vec<u8>, LookupArtifactError> {
        Err(LookupArtifactError::LookupArtifactNotFound {
            name: name.to_owned(),
        })
    }
}

/// An agent aimed at a port nothing listens on, so every call fails locally.
pub(crate) fn unreachable_agent() -> Agent {
    Agent::builder()
        .with_url("http://127.0.0.1:1")
        .build()
        .expect("agent with a well-formed url should build")
}

/// A minimal canister with no build or sync steps.
pub(crate) fn bare_canister(name: &str) -> Canister {
    Canister {
        name: name.to_string(),
        settings: Settings::default(),
        build: BuildSteps { steps: Vec::new() },
        sync: SyncSteps::default(),
        init_args: None,
        registry_recipe: None,
        bindings: BTreeMap::new(),
        friendly_names: vec![name.to_string()],
        environment_variable_files: BTreeMap::new(),
    }
}

/// The `(outcome, message)` of the single `TaskFinished` event for `id`.
///
/// Panics unless exactly one such event exists, which is itself the assertion that
/// every task is closed out once and only once.
pub(crate) fn outcome_of(events: &[Event], id: TaskId) -> (Outcome, Option<String>) {
    let mut finishes = events.iter().filter_map(|event| match event {
        Event::TaskFinished {
            id: finished,
            outcome,
            message,
        } if *finished == id => Some((*outcome, message.clone())),
        _ => None,
    });

    let finish = finishes.next().unwrap_or_else(|| {
        panic!("no TaskFinished event for {id:?}");
    });
    assert!(finishes.next().is_none(), "{id:?} finished more than once");

    finish
}

/// The labels of every task, in the order the tasks were started.
pub(crate) fn task_labels(events: &[Event]) -> Vec<Option<String>> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::TaskStarted { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect()
}
