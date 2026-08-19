//! Presentation layer for [`icp_events`] streams.
//!
//! Operations emit typed events through a [`icp_events::Reporter`]; a
//! [`Renderer`] consumes the stream and owns everything user-facing: wording,
//! progress bars, and the deferred failure dumps. Commands pick a renderer
//! with [`Renderer::for_ctx`] and drive it with [`Renderer::run`] alongside
//! the operation.

use std::collections::BTreeMap;

use icp_events::{Event, TaskId, TaskKind};
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

// Wording for each task kind. Events carry data; these helpers own the words.

/// Live header shown while a step runs, e.g. "Building: step 1 of 3 (script)…".
/// `label` may span multiple lines.
fn step_header(kind: &TaskKind, number: usize, total: usize, label: &str) -> String {
    match kind {
        TaskKind::Build { .. } => format!("Building: step {number} of {total} {label}"),
    }
}

/// Label for the captured-output header, e.g. "[name] Build output:".
fn output_label(kind: &TaskKind) -> &'static str {
    match kind {
        TaskKind::Build { .. } => "Build",
    }
}

/// Final progress-bar message for a task that succeeded.
fn success_message(kind: &TaskKind) -> String {
    match kind {
        TaskKind::Build { .. } => "Built successfully".to_owned(),
    }
}

/// Final progress-bar message for a task that failed.
fn failure_message(kind: &TaskKind, message: &str) -> String {
    match kind {
        TaskKind::Build { .. } => format!("Failed to build canister: {message}"),
    }
}

/// First line of a task's failure dump.
fn failure_header(kind: &TaskKind) -> String {
    match kind {
        TaskKind::Build { canister } => {
            format!("----- Failed to build canister '{canister}' -----")
        }
    }
}

/// Captured output of one task, kept so a failure can be replayed after the
/// live view is gone.
pub(super) struct TaskLog {
    kind: TaskKind,
    finished_steps: Vec<StepLog>,
    current_step: Option<StepLog>,
    failure: Option<String>,
}

struct StepLog {
    title: String,
    lines: RollingLines,
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

    fn fail(&mut self, message: String) {
        self.failure = Some(message);
    }

    /// Render the captured output. When `all_steps` is true, output from
    /// every step is included; otherwise only the last (failing) step is
    /// shown.
    fn dump(&self, all_steps: bool) -> Vec<String> {
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
    for log in logs.values() {
        let Some(message) = &log.failure else {
            continue;
        };

        error!("{}", failure_header(&log.kind));
        error!("'{message}'");
        for line in log.dump(all_steps) {
            error!("{line}");
        }
    }
}
