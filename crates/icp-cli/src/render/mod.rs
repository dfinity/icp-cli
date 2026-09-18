//! Presentation layer for [`icp_events`] streams.
//!
//! Operations emit typed events through a [`Reporter`]; a [`Renderer`]
//! consumes the stream and owns everything user-facing: wording, progress
//! bars, and the deferred failure dumps. Commands pick a renderer with
//! [`Renderer::for_ctx`] and drive it with [`Renderer::run`] alongside the
//! operation.
//!
//! `icp_events` is generic over the task payload and knows nothing about what
//! is being run; the vocabulary comes from [`icp::operations::task`], where
//! each kind of work describes itself. What lives here is only how those
//! descriptions are drawn on a terminal.
//!
//! Tasks arrive as a tree — a composite operation forwards the tasks of the
//! operations it calls as children of its own phase — so a renderer tracks
//! each task's depth and indents accordingly.

use std::collections::{BTreeMap, VecDeque};

use icp::operations::task::{Failure, Presentation, Task};
use icp_events::{TaskId, TaskOutcome};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::error;

mod interactive;
mod plain;
mod spinner;
mod style;

pub(crate) use interactive::InteractiveRenderer;
pub(crate) use plain::PlainRenderer;
pub(crate) use spinner::{ProgressManager, ProgressManagerSettings};

use icp::operations::task::{Event, Reporter, TaskReporter};

/// The maximum number of lines to display for a step output
const MAX_LINES_PER_STEP: usize = 10_000;

/// Indentation applied per level of task nesting.
const INDENT: &str = "  ";

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

/// Run a single task under its own renderer: starts `task`, hands its
/// reporter to `op`, and finishes the task from the result before the
/// renderer flushes.
pub(crate) async fn rendered_task<T, E: std::fmt::Display>(
    debug: bool,
    task: Task,
    op: impl AsyncFnOnce(&TaskReporter) -> Result<T, E>,
) -> Result<T, E> {
    rendered(debug, async |reporter| {
        let task = reporter.task(task);
        let result = op(&task).await;

        match &result {
            Ok(_) => task.finish(TaskOutcome::succeeded()),
            Err(error) => task.finish(TaskOutcome::failed(error.to_string())),
        }

        result
    })
    .await
}

/// Prefix a line with the canister a task is about, so concurrent tasks stay
/// attributable. A task that names no canister — a phase heading — is left
/// undecorated.
fn attributed(task: &Task, text: &str) -> String {
    match task.presentation().canister() {
        Some(name) => format!("[{name}] {text}"),
        None => text.to_owned(),
    }
}

/// Print output lines a task retained past its rolling step view (e.g.
/// sync-plugin stderr), attributed to the task that produced them.
fn print_retained(task: &Task, lines: &[String]) {
    for line in lines {
        eprintln!("{}", attributed(task, line));
    }
}

/// Captured output of one task, kept so a failure can be replayed after the
/// live view is gone.
pub(super) struct TaskLog {
    task: Task,
    /// Levels of nesting below the root, for indentation.
    depth: usize,
    finished_steps: Vec<StepLog>,
    current_step: Option<StepLog>,
    failure: Option<Failure>,
}

struct StepLog {
    title: String,
    lines: RollingLines,
}

/// A fixed-capacity rolling buffer that always holds the last `capacity` items.
#[derive(Debug)]
struct RollingLines {
    buf: VecDeque<String>,
    capacity: usize,
}

impl RollingLines {
    /// Create a new buffer with a fixed capacity.
    fn new(capacity: usize) -> Self {
        let buf = VecDeque::with_capacity(capacity);
        Self { buf, capacity }
    }

    /// Push a new line, evicting the oldest if full.
    fn push(&mut self, line: String) {
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }

        self.buf.push_back(line);
    }

    /// Get an iterator over the current contents (in order).
    fn iter(&self) -> impl Iterator<Item = &str> {
        self.buf.iter().map(|s| s.as_str())
    }

    /// Whether no lines have been pushed.
    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

impl TaskLog {
    fn new(task: Task, depth: usize) -> Self {
        Self {
            task,
            depth,
            finished_steps: Vec::new(),
            current_step: None,
            failure: None,
        }
    }

    fn task(&self) -> &Task {
        &self.task
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn presentation(&self) -> &dyn Presentation {
        self.task.presentation()
    }

    /// Attribute a line to the canister this task is about.
    fn line(&self, text: &str) -> String {
        attributed(&self.task, text)
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

        let mut lines = vec![self.line(&format!("{} output:", self.presentation().output_label()))];

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
                    lines.push(self.line(&format!("{line}:")));
                }
            }

            if step.lines.is_empty() {
                lines.push(self.line("<no output>"));
            } else {
                lines.extend(
                    step.lines
                        .iter()
                        .map(|line| self.line(&format!("> {line}"))),
                );
            }
        }

        lines
    }
}

/// Print the failure dump for every failed task, in task-creation order.
/// A task whose failure the caller could have proceeded past gets the CLI's
/// wording for how to do that, once, at the end.
fn dump_failures(logs: &BTreeMap<TaskId, TaskLog>, all_steps: bool) {
    let mut bypassable = false;

    for log in logs.values() {
        let Some(failure) = &log.failure else {
            continue;
        };
        let presentation = log.presentation();
        let Some(dump) = presentation.failure_dump(failure) else {
            continue;
        };

        for line in dump {
            error!("{line}");
        }
        for line in log.dump(all_steps) {
            error!("{line}");
        }

        bypassable |= presentation.failure_is_bypassable();
    }

    if bypassable {
        error!("Use --yes to bypass this check.");
    }
}
