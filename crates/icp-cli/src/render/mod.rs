//! Presentation layer for [`icp_events`] streams.
//!
//! Operations emit typed events through a [`icp_events::Reporter`]; a
//! [`Renderer`] consumes the stream and owns everything user-facing: wording,
//! progress bars, and the deferred failure dumps. Commands pick a renderer
//! with [`Renderer::for_ctx`] and drive it with [`Renderer::run`] alongside
//! the operation.

use std::collections::BTreeMap;

use icp_events::{Event, Reporter, TaskId, TaskKind, TaskOutcome, TaskReporter, TransferBlob};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::error;

use crate::progress::{MAX_LINES_PER_STEP, RollingLines};

mod interactive;
mod plain;

pub(crate) use interactive::InteractiveRenderer;
pub(crate) use plain::PlainRenderer;

pub(crate) enum Renderer {
    Interactive(InteractiveRenderer),
    Plain(PlainRenderer),
}

impl Renderer {
    /// Pick the renderer matching how the CLI was invoked: live progress bars
    /// normally, plain output under `--debug` (where indicatif bars would
    /// interleave with the debug log).
    pub(crate) fn for_ctx(debug: bool) -> Self {
        if debug {
            Renderer::Plain(PlainRenderer::new())
        } else {
            Renderer::Interactive(InteractiveRenderer::new())
        }
    }

    /// Drive the renderer until every reporter handle is dropped, then flush
    /// deferred output (the per-task failure dumps).
    pub(crate) async fn run(self, mut events: UnboundedReceiver<Event>) {
        match self {
            Renderer::Interactive(mut renderer) => {
                while let Some(event) = events.recv().await {
                    renderer.handle(event);
                }
                renderer.flush();
            }
            Renderer::Plain(mut renderer) => {
                while let Some(event) = events.recv().await {
                    renderer.handle(event);
                }
                renderer.flush();
            }
        }
    }
}

/// Run one operation phase with a fresh event channel and a renderer driving
/// its display: the reporter is handed to `op`, and once `op` finishes the
/// stream is closed and the renderer flushes (failure dumps) before the
/// operation's result is returned.
pub(crate) async fn rendered<T>(debug: bool, op: impl AsyncFnOnce(&Reporter) -> T) -> T {
    let (reporter, events) = icp_events::channel();
    let render = tokio::spawn(Renderer::for_ctx(debug).run(events));

    let result = op(&reporter).await;

    drop(reporter);
    render.await.expect("renderer task panicked");

    result
}

/// Run a single task under its own renderer: starts a task of `kind`, hands
/// its reporter to `op`, and finishes the task from the result before the
/// renderer flushes.
pub(crate) async fn rendered_task<T, E: std::fmt::Display>(
    debug: bool,
    kind: TaskKind,
    op: impl AsyncFnOnce(&TaskReporter) -> Result<T, E>,
) -> Result<T, E> {
    rendered(debug, async |reporter| {
        let task = reporter.task(kind);
        let result = op(&task).await;

        match &result {
            Ok(_) => task.finish(TaskOutcome::succeeded()),
            Err(error) => task.finish(TaskOutcome::failed(error.to_string())),
        }

        result
    })
    .await
}

// Wording for each task kind. Events carry data; these helpers own the words.

/// Message shown while a task runs, before any step reports in. Multi-step
/// tasks (build, sync) have none — their step headers take over.
fn running_message(kind: &TaskKind) -> Option<&'static str> {
    match kind {
        // Build and sync step headers take over; a transfer's byte bar has no
        // message slot at all.
        TaskKind::Build { .. } | TaskKind::Sync { .. } | TaskKind::SnapshotTransfer { .. } => None,
        TaskKind::Create { .. } => Some("Creating..."),
        TaskKind::Install { .. } => Some("Installing..."),
        TaskKind::UpdateSettings { .. } => Some("Updating canister settings..."),
        TaskKind::UpdateEnvironmentVariables { .. } => Some("Updating environment variables..."),
        TaskKind::CandidCheck { .. } => Some("Checking compatibility..."),
    }
}

/// Prefix label for a snapshot-transfer byte bar.
fn transfer_label(blob: &TransferBlob) -> &'static str {
    match blob {
        TransferBlob::WasmModule => "WASM module",
        TransferBlob::WasmMemory => "WASM memory",
        TransferBlob::StableMemory => "Stable memory",
    }
}

/// Live header shown while a step runs, e.g. "Building: step 1 of 3 (script)…".
/// `label` may span multiple lines.
fn step_header(kind: &TaskKind, number: usize, total: usize, label: &str) -> String {
    match kind {
        TaskKind::Sync { .. } => format!("\nSyncing: {label} {number} of {total}"),
        // Only build and sync report steps; a generic header for the rest.
        _ => format!("Building: step {number} of {total} {label}"),
    }
}

/// Label for the captured-output header, e.g. "[name] Build output:".
fn output_label(kind: &TaskKind) -> &'static str {
    match kind {
        TaskKind::Sync { .. } => "Sync",
        // Only build and sync capture step output; a generic label for the rest.
        _ => "Build",
    }
}

/// Final progress-bar message for a task that succeeded.
fn success_message(kind: &TaskKind) -> String {
    match kind {
        TaskKind::Build { .. } => "Built successfully".to_owned(),
        TaskKind::Sync { canister_id, .. } => format!("Synced successfully: {canister_id}"),
        TaskKind::Create { .. } => "Created successfully".to_owned(),
        TaskKind::Install { .. } => "Installed successfully".to_owned(),
        TaskKind::UpdateSettings { .. } => "Canister settings updated successfully".to_owned(),
        TaskKind::UpdateEnvironmentVariables { .. } => {
            "Environment variables updated successfully".to_owned()
        }
        TaskKind::CandidCheck { .. } => "Compatible".to_owned(),
        // A transfer's byte bar has no message slot; nothing to show.
        TaskKind::SnapshotTransfer { .. } => "done".to_owned(),
    }
}

/// Final progress-bar message for a task that failed.
fn failure_message(kind: &TaskKind, message: &str) -> String {
    match kind {
        TaskKind::Build { .. } => format!("Failed to build canister: {message}"),
        TaskKind::Sync { .. } => format!("Failed to sync canister: {message}"),
        // Create failures surface through the command's returned error; the
        // bar shows the bare message.
        TaskKind::Create { .. } => message.to_owned(),
        TaskKind::Install { .. } => format!("Failed to install canister: {message}"),
        TaskKind::UpdateSettings { .. } => {
            format!("Failed to update canister settings: {message}")
        }
        TaskKind::UpdateEnvironmentVariables { .. } => {
            format!("Failed to update environment variables: {message}")
        }
        TaskKind::CandidCheck { .. } => "Incompatible".to_owned(),
        // Transfer failures surface through the command's returned error.
        TaskKind::SnapshotTransfer { .. } => message.to_owned(),
    }
}

/// First line of a task's failure dump, or `None` for kinds that don't get a
/// deferred dump (their failure travels on the command's returned error).
fn failure_header(kind: &TaskKind) -> Option<String> {
    match kind {
        TaskKind::Build { canister } => {
            Some(format!("----- Failed to build canister '{canister}' -----"))
        }
        TaskKind::Sync {
            canister,
            canister_id,
        } => Some(format!(
            "----- Failed to sync canister '{canister}': {canister_id} -----"
        )),
        TaskKind::Create { .. } => None,
        TaskKind::Install {
            canister,
            canister_id,
        } => Some(format!(
            "----- Failed to install canister '{canister}': {canister_id} -----"
        )),
        TaskKind::UpdateSettings {
            canister,
            canister_id,
        } => Some(format!(
            "----- Failed to update settings for canister '{canister}': {canister_id} -----"
        )),
        TaskKind::UpdateEnvironmentVariables {
            canister,
            canister_id,
        } => Some(format!(
            "----- Failed to update environment variables for canister '{canister}': {canister_id} -----"
        )),
        TaskKind::CandidCheck {
            canister,
            canister_id,
        } => Some(format!(
            " ----- Candid interface compatibility check failed: '{canister}' ({canister_id}) -----"
        )),
        TaskKind::SnapshotTransfer { .. } => None,
    }
}

/// Print output lines a task retained past its rolling step view (e.g.
/// sync-plugin stderr), prefixed with the canister name.
fn print_retained(kind: &TaskKind, lines: &[String]) {
    for line in lines {
        eprintln!("[{}] {line}", kind.canister());
    }
}

/// Captured output of one task, kept so a failure can be replayed after the
/// live view is gone.
pub(super) struct TaskLog {
    kind: TaskKind,
    finished_steps: Vec<StepLog>,
    current_step: Option<StepLog>,
    failure: Option<Failure>,
}

struct StepLog {
    title: String,
    lines: RollingLines,
}

struct Failure {
    message: String,
    causes: Vec<String>,
}

impl TaskLog {
    fn new(kind: TaskKind) -> Self {
        Self {
            kind,
            finished_steps: Vec::new(),
            current_step: None,
            failure: None,
        }
    }

    fn kind(&self) -> &TaskKind {
        &self.kind
    }

    fn start_step(&mut self, title: String) {
        self.end_step();
        self.current_step = Some(StepLog {
            title,
            // We need _some_ limit to prevent consuming infinite memory
            lines: RollingLines::new(MAX_LINES_PER_STEP),
        });
    }

    fn push_line(&mut self, line: String) {
        if let Some(step) = &mut self.current_step {
            step.lines.push(line);
        }
    }

    fn end_step(&mut self) {
        if let Some(step) = self.current_step.take() {
            self.finished_steps.push(step);
        }
    }

    fn fail(&mut self, message: String, causes: Vec<String>) {
        self.failure = Some(Failure { message, causes });
    }

    /// Render the captured output. When `all_steps` is true, output from
    /// every step is included; otherwise only the last (failing) step is
    /// shown. Tasks that never reported a step (the single-action kinds)
    /// have nothing to replay.
    fn dump(&self, all_steps: bool) -> Vec<String> {
        if self.finished_steps.is_empty() && self.current_step.is_none() {
            return Vec::new();
        }

        let name = self.kind.canister();
        let mut lines = Vec::new();

        lines.push(format!("[{name}] {} output:", output_label(&self.kind)));

        let steps: &[StepLog] = if all_steps {
            &self.finished_steps
        } else {
            self.finished_steps
                .last()
                .map(std::slice::from_ref)
                .unwrap_or_default()
        };

        for step in steps {
            for line in step.title.lines() {
                if !line.is_empty() {
                    lines.push(format!("[{name}] {line}:"));
                }
            }

            if step.lines.is_empty() {
                lines.push(format!("[{name}] <no output>"));
            } else {
                lines.extend(step.lines.iter().map(|line| format!("[{name}] > {line}")));
            }
        }

        lines
    }
}

/// Print the failure dump for every failed task, in task-creation order.
fn dump_failures(logs: &BTreeMap<TaskId, TaskLog>, all_steps: bool) {
    let mut candid_failures = false;

    for log in logs.values() {
        let Some(failure) = &log.failure else {
            continue;
        };
        let Some(header) = failure_header(&log.kind) else {
            continue;
        };

        error!("{header}");
        match &log.kind {
            TaskKind::CandidCheck { .. } => {
                candid_failures = true;
                error!(
                    "You are making a BREAKING change. Other canisters or frontend clients \
                     relying on your canister may stop working.\n\n{}",
                    failure.message,
                );
            }
            _ => {
                error!("'{}'", failure.message);
                for cause in &failure.causes {
                    error!("  caused by: {cause}");
                }
            }
        }
        for line in log.dump(all_steps) {
            error!("{line}");
        }
    }

    if candid_failures {
        error!("Use --yes to bypass this check.");
    }
}
