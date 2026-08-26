//! Progress events passed from operations to a presentation layer.
//!
//! Operations (and the core library underneath them) emit [`Event`]s through
//! cheap-to-clone reporter handles ([`Reporter`] → [`TaskReporter`] →
//! [`StepReporter`]); a consumer reads the stream and decides how to display
//! it.
//!
//! The crate is deliberately ignorant of *what* the work is. A task announces
//! itself with a [`Task`] descriptor — a title, an optional subject, and the
//! [`Shape`] of the widget it drives — and nothing here enumerates builds,
//! syncs or installs. That keeps the operation vocabulary out of every crate
//! that merely reports progress, and lets a host that has no terminal at all
//! (a deploy running inside a canister, say) consume the same stream.
//!
//! Chrome is the consumer's job: brackets, dashes, tick marks, indentation
//! and colour are all applied by whoever renders the stream. What travels on
//! the wire is the operation's own nouns.
//!
//! Sends never block and never fail: the channel is unbounded, and with no
//! receiver events are simply dropped, so tests and headless callers get
//! silence for free. Errors do not travel on this stream — an operation's
//! `Result` remains the source of truth; [`TaskOutcome::Failed`] exists only
//! so a consumer can paint the failure state.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use serde::Serialize;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Identifies one task within an event stream. Ids are assigned in
/// task-creation order, so consumers can present tasks in a stable order
/// regardless of completion order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TaskId(u64);

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub task_id: TaskId,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    /// The task began. Emitted once per task, before any of its steps.
    ///
    /// `parent` is set when this task was spawned from another task's
    /// reporter, which is how a composite operation (deploy calling build,
    /// install, sync, …) forwards its children's progress onto one stream.
    TaskStarted {
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<TaskId>,
        /// What the work is being done *to* — a canister name, typically.
        /// Consumers use it to attribute output lines.
        #[serde(skip_serializing_if = "Option::is_none")]
        subject: Option<String>,
        /// What the work *is*, in the operation's own words, phrased so it
        /// reads as work in progress: "Building", "Checking compatibility".
        title: String,
        #[serde(flatten)]
        shape: Shape,
    },

    /// A step of the task began. Steps within a task are sequential;
    /// `number` is 1-based. `label` describes the step (it may span
    /// multiple lines).
    StepStarted {
        number: usize,
        total: usize,
        label: String,
    },

    /// A shell command within the task's current step began executing.
    /// Script steps run their commands in order; consumers can use this to
    /// attribute the output that follows to the command producing it.
    CommandStarted { command: String },

    /// One line of output produced while the task's current step runs.
    Output { stream: OutputStream, line: String },

    /// How far a quantifiable task has come, against the `total` its
    /// [`Shape::Counter`] declared.
    Progress { position: u64 },

    /// The task's current step finished.
    StepCompleted { outcome: StepOutcome },

    /// The task finished; no further events follow for this task.
    TaskCompleted { outcome: TaskOutcome },
}

/// The kind of widget a task drives — the one presentation fact an operation
/// legitimately knows, because it is a property of the work rather than of
/// the display.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Shape {
    /// An announcement with no live state of its own. It titles whatever
    /// nests beneath it, and a childless one is simply a notice.
    Group,
    /// Work of unknown duration.
    Spinner,
    /// Quantifiable work, measured against `total` (bytes, today).
    Counter { total: u64 },
}

/// How a task announces itself. Built by the operation and handed to
/// [`Reporter::task`] or [`TaskReporter::subtask`].
#[derive(Debug, Clone)]
pub struct Task {
    subject: Option<String>,
    title: String,
    shape: Shape,
}

impl Task {
    /// Work of unknown duration, e.g. an install.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            subject: None,
            title: title.into(),
            shape: Shape::Spinner,
        }
    }

    /// An announcement that titles the tasks nested under it — a deploy
    /// phase, say. With no children it is just a notice, so its title is
    /// rendered verbatim rather than being decorated.
    pub fn group(title: impl Into<String>) -> Self {
        Self {
            subject: None,
            title: title.into(),
            shape: Shape::Group,
        }
    }

    /// Quantifiable work, reported through [`TaskReporter::progress`].
    pub fn counter(title: impl Into<String>, total: u64) -> Self {
        Self {
            subject: None,
            title: title.into(),
            shape: Shape::Counter { total },
        }
    }

    /// Name what the work is being done to, so output can be attributed to it.
    pub fn on(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }
}

/// Where an output line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
    /// A progress note from icp itself rather than a spawned tool.
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum TaskOutcome {
    Succeeded {
        /// Closing line for the task's widget. Left `None` when the operation
        /// has nothing to add beyond "it worked", or when the widget has no
        /// message slot to put it in.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        /// Output lines that belong on the persistent output channel after
        /// success — e.g. sync-plugin stderr, which a rolling step view would
        /// otherwise discard. Most tasks retain nothing.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        retained_output: Vec<String>,
    },
    /// Failure descriptions are for display only; the typed error stays on
    /// the operation's return path.
    Failed(Failure),
    /// The task did not apply and no work was done (e.g. a Candid
    /// compatibility check on an install that is not an upgrade).
    Skipped { reason: String },
}

/// Everything a consumer needs to paint a failure. None of it is load-bearing
/// — the operation's `Err` is.
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    /// Short description, kept terse enough for a progress bar.
    pub message: String,
    /// The rendered `source()` chain of the failure, outermost first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<String>,
    /// Whether, and how, this failure is replayed once the live view is gone.
    #[serde(flatten)]
    pub report: FailureReport,
    /// A note printed once after every dump, when at least one task failed
    /// asking for it. Deduplicated across tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epilogue: Option<String>,
}

/// What a failed task leaves behind after the live view is torn down.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "report", rename_all = "snake_case")]
pub enum FailureReport {
    /// Nothing: the failure reaches the user on the command's returned error,
    /// and a dump would only say it twice.
    Silent,
    /// The message, then its cause chain.
    Summary,
    /// These lines in place of the message and causes — for a failure whose
    /// real content is a report rather than a sentence.
    Detail { lines: Vec<String> },
}

impl TaskOutcome {
    /// Success with nothing to say and nothing retained.
    pub fn succeeded() -> Self {
        TaskOutcome::Succeeded {
            message: None,
            retained_output: Vec::new(),
        }
    }

    /// Success with a closing line for the task's widget.
    pub fn succeeded_with(message: impl Into<String>) -> Self {
        TaskOutcome::Succeeded {
            message: Some(message.into()),
            retained_output: Vec::new(),
        }
    }

    /// Failure summarised by `message`, with no cause chain.
    pub fn failed(message: impl Into<String>) -> Self {
        TaskOutcome::Failed(Failure::new(message))
    }

    /// Failure that the command's returned error already reports, so it needs
    /// no deferred dump of its own.
    pub fn failed_silently(message: impl Into<String>) -> Self {
        TaskOutcome::Failed(Failure {
            report: FailureReport::Silent,
            ..Failure::new(message)
        })
    }
}

impl Failure {
    /// A failure summarised by `message` alone.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            causes: Vec::new(),
            report: FailureReport::Summary,
            epilogue: None,
        }
    }

    /// Attach the rendered `source()` chain, outermost first.
    pub fn with_causes(mut self, causes: Vec<String>) -> Self {
        self.causes = causes;
        self
    }

    /// Replace the dumped summary with `lines`.
    pub fn with_detail(mut self, lines: Vec<String>) -> Self {
        self.report = FailureReport::Detail { lines };
        self
    }

    /// Add a note printed once after all dumps.
    pub fn with_epilogue(mut self, epilogue: impl Into<String>) -> Self {
        self.epilogue = Some(epilogue.into());
        self
    }
}

/// Create a connected reporter/receiver pair. The receiver yields `None` once
/// the reporter and every handle derived from it have been dropped.
pub fn channel() -> (Reporter, UnboundedReceiver<Event>) {
    let (tx, rx) = unbounded_channel();
    let reporter = Reporter {
        inner: Some(ReporterInner {
            tx,
            next_task_id: Arc::new(AtomicU64::new(0)),
        }),
        parent: None,
    };
    (reporter, rx)
}

/// Create a lone [`StepReporter`] wired to its own receiver, for callers that
/// need to observe a single step's output without the task/step ceremony —
/// primarily tests.
pub fn step_channel() -> (StepReporter, UnboundedReceiver<Event>) {
    let (tx, rx) = unbounded_channel();
    (
        StepReporter {
            tx: Some(tx),
            task_id: TaskId(0),
        },
        rx,
    )
}

/// Entry point handed to an operation; spawns [`TaskReporter`]s.
///
/// A reporter carries the scope it was made in. An operation cannot tell the
/// difference between the reporter a command handed it and one scoped to a
/// parent task by a composite operation — which is what lets `deploy` run
/// `build_many` unmodified and have its tasks nest under the build phase.
#[derive(Debug, Clone)]
pub struct Reporter {
    inner: Option<ReporterInner>,
    /// Task the reporter's tasks nest under, if any.
    parent: Option<TaskId>,
}

#[derive(Debug, Clone)]
struct ReporterInner {
    tx: UnboundedSender<Event>,
    next_task_id: Arc<AtomicU64>,
}

impl ReporterInner {
    fn start(&self, parent: Option<TaskId>, task: Task) -> TaskReporter {
        let task_id = TaskId(self.next_task_id.fetch_add(1, Ordering::Relaxed));
        let _ = self.tx.send(Event {
            task_id,
            kind: EventKind::TaskStarted {
                parent,
                subject: task.subject,
                title: task.title,
                shape: task.shape,
            },
        });
        TaskReporter {
            inner: Some(self.clone()),
            task_id,
        }
    }
}

impl Reporter {
    /// A reporter whose events go nowhere.
    pub fn null() -> Self {
        Self {
            inner: None,
            parent: None,
        }
    }

    /// Begin a task, emitting [`EventKind::TaskStarted`]. It nests under
    /// whatever scope this reporter carries.
    pub fn task(&self, task: Task) -> TaskReporter {
        match &self.inner {
            Some(inner) => inner.start(self.parent, task),
            None => TaskReporter::null(),
        }
    }

    /// Announce something in passing: a task with no work under it, finished
    /// as soon as it is started.
    pub fn notice(&self, text: impl Into<String>) {
        self.task(Task::group(text))
            .finish(TaskOutcome::succeeded());
    }
}

/// Reports the lifecycle of one task, and spawns the tasks nested under it.
#[derive(Debug, Clone)]
pub struct TaskReporter {
    inner: Option<ReporterInner>,
    task_id: TaskId,
}

impl TaskReporter {
    /// A task reporter whose events go nowhere.
    pub fn null() -> Self {
        Self {
            inner: None,
            task_id: TaskId(0),
        }
    }

    /// Begin a task nested under this one. A composite operation uses this to
    /// forward the progress of the operations it calls onto one stream.
    pub fn subtask(&self, task: Task) -> TaskReporter {
        match &self.inner {
            Some(inner) => inner.start(Some(self.task_id), task),
            None => TaskReporter::null(),
        }
    }

    /// A reporter scoped to this task. Hand it to an operation that does not
    /// know it is being composed: whatever tasks it starts nest here.
    pub fn reporter(&self) -> Reporter {
        Reporter {
            inner: self.inner.clone(),
            parent: Some(self.task_id),
        }
    }

    fn send(&self, kind: EventKind) {
        if let Some(inner) = &self.inner {
            let _ = inner.tx.send(Event {
                task_id: self.task_id,
                kind,
            });
        }
    }

    /// Begin the task's next step, emitting [`EventKind::StepStarted`].
    /// `number` is 1-based.
    pub fn step(&self, number: usize, total: usize, label: impl Into<String>) -> StepReporter {
        self.send(EventKind::StepStarted {
            number,
            total,
            label: label.into(),
        });
        StepReporter {
            tx: self.inner.as_ref().map(|inner| inner.tx.clone()),
            task_id: self.task_id,
        }
    }

    /// Report how far the task has come, against the total its
    /// [`Shape::Counter`] declared.
    pub fn progress(&self, position: u64) {
        self.send(EventKind::Progress { position });
    }

    /// Emit a line of output attributed to this task rather than to a step.
    pub fn output(&self, stream: OutputStream, line: impl Into<String>) {
        self.send(EventKind::Output {
            stream,
            line: line.into(),
        });
    }

    /// Emit a progress note from icp itself.
    pub fn info(&self, line: impl Into<String>) {
        self.output(OutputStream::Info, line);
    }

    /// Finish the task, emitting [`EventKind::TaskCompleted`]. No further
    /// events should be sent for this task.
    pub fn finish(&self, outcome: TaskOutcome) {
        self.send(EventKind::TaskCompleted { outcome });
    }
}

/// Reports output produced during one step of a task.
///
/// This handle carries no task vocabulary at all, which is what lets the core
/// library's ports (`Arc<dyn Build>`, `Arc<dyn Synchronize>`) accept one.
#[derive(Debug, Clone)]
pub struct StepReporter {
    tx: Option<UnboundedSender<Event>>,
    task_id: TaskId,
}

impl StepReporter {
    /// A step reporter whose events go nowhere.
    pub fn null() -> Self {
        Self {
            tx: None,
            task_id: TaskId(0),
        }
    }

    fn send(&self, kind: EventKind) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Event {
                task_id: self.task_id,
                kind,
            });
        }
    }

    /// Report that a shell command within the step began executing, emitting
    /// [`EventKind::CommandStarted`].
    pub fn command(&self, command: impl Into<String>) {
        self.send(EventKind::CommandStarted {
            command: command.into(),
        });
    }

    /// Emit one line of output.
    pub fn output(&self, stream: OutputStream, line: impl Into<String>) {
        self.send(EventKind::Output {
            stream,
            line: line.into(),
        });
    }

    /// Emit one line of tool stdout.
    pub fn stdout(&self, line: impl Into<String>) {
        self.output(OutputStream::Stdout, line);
    }

    /// Emit one line of tool stderr.
    pub fn stderr(&self, line: impl Into<String>) {
        self.output(OutputStream::Stderr, line);
    }

    /// Emit a progress note from icp itself.
    pub fn info(&self, line: impl Into<String>) {
        self.output(OutputStream::Info, line);
    }

    /// Finish the step, emitting [`EventKind::StepCompleted`].
    pub fn done(&self, outcome: StepOutcome) {
        self.send(EventKind::StepCompleted { outcome });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mut UnboundedReceiver<Event>) -> Vec<Event> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn null_reporters_emit_nothing_and_never_panic() {
        let reporter = Reporter::null();
        let task = reporter.task(Task::new("Working").on("thing"));
        let child = task.subtask(Task::new("Nested"));
        let step = task.step(1, 1, "only");

        // Every method must be safe to call with no receiver attached.
        step.command("echo hi");
        step.stdout("out");
        step.stderr("err");
        step.info("note");
        step.done(StepOutcome::Succeeded);
        task.progress(42);
        task.info("note");
        child.finish(TaskOutcome::succeeded());
        task.finish(TaskOutcome::succeeded());

        // Handles derived from a null reporter are themselves null.
        assert!(TaskReporter::null().inner.is_none());
        assert!(StepReporter::null().tx.is_none());
        assert!(task.reporter().inner.is_none());
    }

    #[tokio::test]
    async fn task_ids_follow_creation_order_across_nesting() {
        let (reporter, mut rx) = channel();
        let parent = reporter.task(Task::group("Phase"));
        let first = parent.subtask(Task::new("A"));
        let second = parent.subtask(Task::new("B"));

        // Finishing out of order must not disturb the ids.
        second.finish(TaskOutcome::succeeded());
        first.finish(TaskOutcome::succeeded());
        parent.finish(TaskOutcome::succeeded());

        let started: Vec<(TaskId, Option<TaskId>)> = drain(&mut rx)
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::TaskStarted { parent, .. } => Some((e.task_id, parent)),
                _ => None,
            })
            .collect();
        assert_eq!(
            started,
            vec![
                (TaskId(0), None),
                (TaskId(1), Some(TaskId(0))),
                (TaskId(2), Some(TaskId(0))),
            ]
        );
    }

    /// A composite operation hands plain `Reporter`s to the operations it
    /// calls; those must still land in the same stream, nested under it.
    #[tokio::test]
    async fn a_task_can_hand_out_a_reporter_for_uncomposed_callees() {
        let (reporter, mut rx) = channel();
        let phase = reporter.task(Task::group("Phase"));
        let inner = phase.reporter();
        inner
            .task(Task::new("Callee"))
            .finish(TaskOutcome::succeeded());

        let started: Vec<(String, Option<TaskId>)> = drain(&mut rx)
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::TaskStarted { title, parent, .. } => Some((title, parent)),
                _ => None,
            })
            .collect();
        assert_eq!(
            started,
            vec![
                ("Phase".to_owned(), None),
                // The callee asked for a top-level task and got a child of
                // the phase, without knowing the difference.
                ("Callee".to_owned(), Some(TaskId(0))),
            ]
        );
    }

    #[tokio::test]
    async fn events_arrive_in_emission_order_and_carry_their_task_id() {
        let (reporter, mut rx) = channel();
        let task = reporter.task(Task::new("Building").on("frontend"));
        let step = task.step(1, 2, "compile");
        step.command("make");
        step.stdout("line one");
        step.stderr("line two");
        step.done(StepOutcome::Succeeded);
        task.finish(TaskOutcome::succeeded());

        let events = drain(&mut rx);
        assert!(events.iter().all(|e| e.task_id == TaskId(0)));

        let shape: Vec<String> = events
            .iter()
            .map(|e| match &e.kind {
                EventKind::TaskStarted { title, .. } => format!("started {title}"),
                EventKind::StepStarted {
                    number,
                    total,
                    label,
                } => format!("step {number}/{total} {label}"),
                EventKind::CommandStarted { command } => format!("$ {command}"),
                EventKind::Output { stream, line } => format!("{stream:?} {line}"),
                EventKind::Progress { position } => format!("progress {position}"),
                EventKind::StepCompleted { .. } => "step done".to_owned(),
                EventKind::TaskCompleted { .. } => "task done".to_owned(),
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                "started Building",
                "step 1/2 compile",
                "$ make",
                "Stdout line one",
                "Stderr line two",
                "step done",
                "task done",
            ]
        );
    }

    /// A step reporter keeps its own channel handle, so output emitted after
    /// the task reporter is gone still arrives. This is what makes the
    /// spawned stdout/stderr readers in the core library safe.
    #[tokio::test]
    async fn step_reporter_outlives_its_task_reporter() {
        let (reporter, mut rx) = channel();
        let step = {
            let task = reporter.task(Task::new("Working"));
            task.step(1, 1, "run")
        };
        step.stdout("after the task handle dropped");

        let lines: Vec<String> = drain(&mut rx)
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::Output { line, .. } => Some(line),
                _ => None,
            })
            .collect();
        assert_eq!(lines, vec!["after the task handle dropped".to_owned()]);
    }

    /// The receiver must close once every handle is gone, otherwise a
    /// consumer driving the stream to completion would hang.
    #[tokio::test]
    async fn stream_closes_when_all_handles_drop() {
        let (reporter, mut rx) = channel();
        let task = reporter.task(Task::new("Working"));
        let step = task.step(1, 1, "run");

        drop(reporter);
        drop(task);
        assert!(rx.recv().await.is_some(), "buffered events still deliver");

        drop(step);
        while rx.recv().await.is_some() {}
        // Reaching here means recv() returned None rather than hanging.
    }

    /// The stream is the CLI's structured account of a run, so its tags and
    /// field names are pinned here rather than left to chance.
    #[test]
    fn wire_format_is_stable() {
        let event = |kind| {
            serde_json::to_value(Event {
                task_id: TaskId(7),
                kind,
            })
            .expect("event should serialize")
        };

        assert_eq!(
            event(EventKind::TaskStarted {
                parent: Some(TaskId(2)),
                subject: Some("frontend".to_owned()),
                title: "Building".to_owned(),
                shape: Shape::Spinner,
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_started", "parent": 2,
                "subject": "frontend", "title": "Building", "shape": "spinner",
            })
        );
        // A top-level task with no subject omits both fields rather than
        // sending nulls.
        assert_eq!(
            event(EventKind::TaskStarted {
                parent: None,
                subject: None,
                title: "Building canisters:".to_owned(),
                shape: Shape::Group,
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_started",
                "title": "Building canisters:", "shape": "group",
            })
        );
        assert_eq!(
            event(EventKind::TaskStarted {
                parent: None,
                subject: None,
                title: "WASM module".to_owned(),
                shape: Shape::Counter { total: 4096 },
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_started",
                "title": "WASM module", "shape": "counter", "total": 4096,
            })
        );
        assert_eq!(
            event(EventKind::StepStarted {
                number: 1,
                total: 3,
                label: "compile".to_owned(),
            }),
            serde_json::json!({
                "task_id": 7, "event": "step_started",
                "number": 1, "total": 3, "label": "compile",
            })
        );
        assert_eq!(
            event(EventKind::CommandStarted {
                command: "make build".to_owned(),
            }),
            serde_json::json!({
                "task_id": 7, "event": "command_started", "command": "make build",
            })
        );
        assert_eq!(
            event(EventKind::Output {
                stream: OutputStream::Stderr,
                line: "boom".to_owned(),
            }),
            serde_json::json!({
                "task_id": 7, "event": "output", "stream": "stderr", "line": "boom",
            })
        );
        assert_eq!(
            event(EventKind::Progress { position: 1024 }),
            serde_json::json!({ "task_id": 7, "event": "progress", "position": 1024 })
        );
        assert_eq!(
            event(EventKind::StepCompleted {
                outcome: StepOutcome::Failed,
            }),
            serde_json::json!({ "task_id": 7, "event": "step_completed", "outcome": "failed" })
        );

        // Task outcomes nest under `outcome`, internally tagged by `result`.
        assert_eq!(
            event(EventKind::TaskCompleted {
                outcome: TaskOutcome::succeeded(),
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": { "result": "succeeded" },
            })
        );
        assert_eq!(
            event(EventKind::TaskCompleted {
                outcome: TaskOutcome::Succeeded {
                    message: Some("Built successfully".to_owned()),
                    retained_output: vec!["kept".to_owned()],
                },
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": {
                    "result": "succeeded", "message": "Built successfully",
                    "retained_output": ["kept"],
                },
            })
        );
        assert_eq!(
            event(EventKind::TaskCompleted {
                outcome: TaskOutcome::Failed(
                    Failure::new("no")
                        .with_causes(vec!["because".to_owned()])
                        .with_epilogue("try harder"),
                ),
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": {
                    "result": "failed", "message": "no", "causes": ["because"],
                    "report": "summary", "epilogue": "try harder",
                },
            })
        );
        assert_eq!(
            event(EventKind::TaskCompleted {
                outcome: TaskOutcome::Failed(
                    Failure::new("no").with_detail(vec!["why".to_owned()])
                ),
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": {
                    "result": "failed", "message": "no",
                    "report": "detail", "lines": ["why"],
                },
            })
        );
        assert_eq!(
            event(EventKind::TaskCompleted {
                outcome: TaskOutcome::failed_silently("no"),
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": { "result": "failed", "message": "no", "report": "silent" },
            })
        );
        assert_eq!(
            event(EventKind::TaskCompleted {
                outcome: TaskOutcome::Skipped {
                    reason: "not an upgrade".to_owned(),
                },
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": { "result": "skipped", "reason": "not an upgrade" },
            })
        );
    }
}
