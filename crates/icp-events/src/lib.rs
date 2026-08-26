//! Typed progress events passed from operations to the presentation layer.
//!
//! Operations emit [`Event`]s through cheap-to-clone reporter handles
//! ([`Reporter`] → [`TaskReporter`] → [`StepReporter`]); a consumer reads the
//! event stream and decides how to display it. Events carry data, not prose —
//! wording, layout, and color belong to the consumer.
//!
//! The crate is deliberately ignorant of *what* a task is. [`Event`] is
//! generic over a task payload `T` supplied by the caller, so the vocabulary
//! of operations (build, sync, install, …) lives with the code that renders
//! it rather than here. [`StepReporter`], by contrast, is **not** generic: it
//! erases `T` behind [`StepSink`] so that ports in the core library — which
//! are stored as `Arc<dyn Build>` / `Arc<dyn Synchronize>` — can accept one
//! without naming the consumer's task type.
//!
//! Tasks form a tree: [`TaskReporter::reporter`] hands back a [`Reporter`]
//! whose tasks nest under that task. An operation cannot tell such a reporter
//! from the one a command would have given it, which is what lets a composite
//! operation forward the progress of the operations it calls onto one stream
//! without those operations changing at all.
//!
//! Sends never block and never fail: the channel is unbounded, and with no
//! receiver events are simply dropped, so tests and headless callers get
//! silence for free. Errors do not travel on this stream — an operation's
//! `Result` remains the source of truth; [`TaskOutcome::Failed`] exists only
//! so a consumer can paint the failure state.

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::Serialize;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Identifies one task (one unit of work) within an event stream. Ids are
/// assigned in task-creation order, so consumers can use them to present
/// tasks in a stable order regardless of completion order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct TaskId(u64);

#[derive(Debug, Clone, Serialize)]
pub struct Event<T> {
    pub task_id: TaskId,
    #[serde(flatten)]
    pub kind: EventKind<T>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind<T> {
    /// The task began. Emitted once per task, before any of its steps. `task`
    /// is the caller's own description of the work — this crate never
    /// inspects it.
    ///
    /// `parent` is set when the task was started through a reporter scoped to
    /// another task, which is how a composite operation (deploy calling
    /// build, install, sync, …) forwards its children's progress onto one
    /// stream.
    TaskStarted {
        #[serde(skip_serializing_if = "Option::is_none")]
        parent: Option<TaskId>,
        task: T,
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

    /// How far a quantifiable task has come, in whatever unit its task
    /// payload declares (e.g. bytes).
    Progress { position: u64 },

    /// The task's current step finished.
    StepCompleted { outcome: StepOutcome },

    /// The task finished; no further events follow for this task.
    TaskCompleted { outcome: TaskOutcome },
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
        /// Output lines that belong on the persistent output channel after
        /// success — e.g. sync-plugin stderr, which a rolling step view would
        /// otherwise discard. Most tasks retain nothing.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        retained_output: Vec<String>,
    },
    /// Failure descriptions are for display only; the typed error stays on
    /// the operation's return path.
    Failed {
        message: String,
        /// The rendered `source()` chain of the failure, outermost first.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        causes: Vec<String>,
    },
    /// The task did not apply and no work was done (e.g. a Candid
    /// compatibility check on an install that is not an upgrade).
    Skipped { reason: String },
}

impl TaskOutcome {
    /// Success with nothing retained.
    pub fn succeeded() -> Self {
        TaskOutcome::Succeeded {
            retained_output: Vec::new(),
        }
    }

    /// Failure with no cause chain.
    pub fn failed(message: impl Into<String>) -> Self {
        TaskOutcome::Failed {
            message: message.into(),
            causes: Vec::new(),
        }
    }
}

/// Create a connected reporter/receiver pair. The receiver yields `None` once
/// the reporter and every handle derived from it have been dropped.
pub fn channel<T: Send + 'static>() -> (Reporter<T>, UnboundedReceiver<Event<T>>) {
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
/// primarily tests. The task payload type is never constructed, so it is
/// usually left as `()`.
pub fn step_channel<T: Send + 'static>() -> (StepReporter, UnboundedReceiver<Event<T>>) {
    let (tx, rx) = unbounded_channel();
    let sink = TaskSink {
        tx,
        task_id: TaskId(0),
    };
    (
        StepReporter {
            sink: Some(Arc::new(sink)),
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
pub struct Reporter<T> {
    inner: Option<ReporterInner<T>>,
    /// Task the reporter's tasks nest under, if any.
    parent: Option<TaskId>,
}

struct ReporterInner<T> {
    tx: UnboundedSender<Event<T>>,
    next_task_id: Arc<AtomicU64>,
}

// Hand-written so cloning a reporter does not require the task payload to be
// `Clone` — only the channel handle is duplicated.
impl<T> Clone for ReporterInner<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            next_task_id: self.next_task_id.clone(),
        }
    }
}

impl<T: Send + 'static> ReporterInner<T> {
    fn start(&self, parent: Option<TaskId>, task: T) -> TaskReporter<T> {
        let task_id = TaskId(self.next_task_id.fetch_add(1, Ordering::Relaxed));
        let _ = self.tx.send(Event {
            task_id,
            kind: EventKind::TaskStarted { parent, task },
        });
        TaskReporter {
            inner: Some(self.clone()),
            task_id,
        }
    }
}

impl<T> Clone for Reporter<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            parent: self.parent,
        }
    }
}

impl<T> Reporter<T> {
    /// A reporter whose events go nowhere.
    pub fn null() -> Self {
        Self {
            inner: None,
            parent: None,
        }
    }
}

impl<T: Send + 'static> Reporter<T> {
    /// Begin a task, emitting [`EventKind::TaskStarted`]. It nests under
    /// whatever scope this reporter carries.
    pub fn task(&self, task: T) -> TaskReporter<T> {
        match &self.inner {
            Some(inner) => inner.start(self.parent, task),
            None => TaskReporter::null(),
        }
    }
}

/// Reports the lifecycle of one task, and spawns the tasks nested under it.
pub struct TaskReporter<T> {
    inner: Option<ReporterInner<T>>,
    task_id: TaskId,
}

impl<T> Clone for TaskReporter<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            task_id: self.task_id,
        }
    }
}

impl<T> TaskReporter<T> {
    /// A task reporter whose events go nowhere.
    pub fn null() -> Self {
        Self {
            inner: None,
            task_id: TaskId(0),
        }
    }

    /// A reporter scoped to this task. Hand it to an operation that does not
    /// know it is being composed: whatever tasks it starts nest here.
    pub fn reporter(&self) -> Reporter<T> {
        Reporter {
            inner: self.inner.clone(),
            parent: Some(self.task_id),
        }
    }

    fn send(&self, kind: EventKind<T>) {
        if let Some(inner) = &self.inner {
            let _ = inner.tx.send(Event {
                task_id: self.task_id,
                kind,
            });
        }
    }

    /// Report how far the task has come, emitting [`EventKind::Progress`].
    /// The unit is whatever the task payload declares.
    pub fn progress(&self, position: u64) {
        self.send(EventKind::Progress { position });
    }

    /// Finish the task, emitting [`EventKind::TaskCompleted`]. No further
    /// events should be sent for this task.
    pub fn finish(&self, outcome: TaskOutcome) {
        self.send(EventKind::TaskCompleted { outcome });
    }
}

impl<T: Send + 'static> TaskReporter<T> {
    /// Begin the task's next step, emitting [`EventKind::StepStarted`].
    /// `number` is 1-based.
    ///
    /// The returned handle has the task payload type erased, so it can be
    /// passed to code that must not know the task vocabulary.
    pub fn step(&self, number: usize, total: usize, label: impl Into<String>) -> StepReporter {
        self.send(EventKind::StepStarted {
            number,
            total,
            label: label.into(),
        });
        StepReporter {
            sink: self.inner.as_ref().map(|inner| {
                Arc::new(TaskSink {
                    tx: inner.tx.clone(),
                    task_id: self.task_id,
                }) as Arc<dyn StepSink>
            }),
        }
    }
}

/// The step-scoped half of an event sink, with the task payload type erased.
///
/// This is the whole reason [`StepReporter`] is not generic: the core
/// library's ports take a step reporter but must not name the task
/// vocabulary, and they are held as trait objects (`Arc<dyn Build>`), which
/// rules out threading a type parameter through them.
trait StepSink: Send + Sync {
    fn command_started(&self, command: String);
    fn output(&self, stream: OutputStream, line: String);
    fn step_completed(&self, outcome: StepOutcome);
}

struct TaskSink<T> {
    tx: UnboundedSender<Event<T>>,
    task_id: TaskId,
}

impl<T: Send + 'static> StepSink for TaskSink<T> {
    fn command_started(&self, command: String) {
        let _ = self.tx.send(Event {
            task_id: self.task_id,
            kind: EventKind::CommandStarted { command },
        });
    }

    fn output(&self, stream: OutputStream, line: String) {
        let _ = self.tx.send(Event {
            task_id: self.task_id,
            kind: EventKind::Output { stream, line },
        });
    }

    fn step_completed(&self, outcome: StepOutcome) {
        let _ = self.tx.send(Event {
            task_id: self.task_id,
            kind: EventKind::StepCompleted { outcome },
        });
    }
}

/// Reports output produced during one step of a task.
#[derive(Clone)]
pub struct StepReporter {
    sink: Option<Arc<dyn StepSink>>,
}

impl fmt::Debug for StepReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepReporter")
            .field("connected", &self.sink.is_some())
            .finish()
    }
}

impl StepReporter {
    /// A step reporter whose events go nowhere.
    pub fn null() -> Self {
        Self { sink: None }
    }

    /// Report that a shell command within the step began executing, emitting
    /// [`EventKind::CommandStarted`].
    pub fn command(&self, command: impl Into<String>) {
        if let Some(sink) = &self.sink {
            sink.command_started(command.into());
        }
    }

    /// Emit one line of output.
    pub fn output(&self, stream: OutputStream, line: impl Into<String>) {
        if let Some(sink) = &self.sink {
            sink.output(stream, line.into());
        }
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
        if let Some(sink) = &self.sink {
            sink.step_completed(outcome);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in task payload. Real callers use their own vocabulary; this
    /// crate only ever moves it across the channel.
    #[derive(Debug, Clone, Serialize)]
    #[serde(tag = "kind", rename = "demo")]
    struct DemoTask {
        name: String,
    }

    fn demo(name: &str) -> DemoTask {
        DemoTask {
            name: name.to_owned(),
        }
    }

    fn drain<T>(rx: &mut UnboundedReceiver<Event<T>>) -> Vec<Event<T>> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn null_reporters_emit_nothing_and_never_panic() {
        let reporter: Reporter<DemoTask> = Reporter::null();
        let task = reporter.task(demo("a"));
        let step = task.step(1, 1, "only");

        // Every method must be safe to call with no receiver attached.
        step.command("echo hi");
        step.stdout("out");
        step.stderr("err");
        step.info("note");
        step.done(StepOutcome::Succeeded);
        task.progress(42);
        task.finish(TaskOutcome::succeeded());

        // Handles derived from a null reporter are themselves null.
        assert!(TaskReporter::<DemoTask>::null().inner.is_none());
        assert!(task.reporter().inner.is_none());
        assert!(StepReporter::null().sink.is_none());
    }

    #[tokio::test]
    async fn task_ids_follow_creation_order() {
        let (reporter, mut rx) = channel::<DemoTask>();
        let first = reporter.task(demo("a"));
        let second = reporter.task(demo("b"));
        let third = reporter.task(demo("c"));

        // Finishing out of order must not disturb the ids.
        third.finish(TaskOutcome::succeeded());
        first.finish(TaskOutcome::succeeded());
        second.finish(TaskOutcome::succeeded());

        let started: Vec<TaskId> = drain(&mut rx)
            .into_iter()
            .filter(|e| matches!(e.kind, EventKind::TaskStarted { .. }))
            .map(|e| e.task_id)
            .collect();
        assert_eq!(started, vec![TaskId(0), TaskId(1), TaskId(2)]);
    }

    #[tokio::test]
    async fn events_arrive_in_emission_order_and_carry_their_task_id() {
        let (reporter, mut rx) = channel::<DemoTask>();
        let task = reporter.task(demo("a"));
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
                EventKind::TaskStarted { .. } => "started".to_owned(),
                EventKind::StepStarted {
                    number,
                    total,
                    label,
                } => {
                    format!("step {number}/{total} {label}")
                }
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
                "started",
                "step 1/2 compile",
                "$ make",
                "Stdout line one",
                "Stderr line two",
                "step done",
                "task done",
            ]
        );
    }

    /// A reporter scoped to a task hands that task's id to everything it
    /// starts, without the operation on the other end knowing. Tasks started
    /// through the unscoped reporter stay at the root.
    #[tokio::test]
    async fn a_scoped_reporter_nests_what_it_starts() {
        let (reporter, mut rx) = channel::<DemoTask>();
        let phase = reporter.task(demo("phase"));
        // What a composed operation receives — indistinguishable, to it, from
        // the reporter a command would have handed it.
        let scoped = phase.reporter();
        scoped.task(demo("child"));
        scoped.clone().task(demo("sibling"));
        reporter.task(demo("root"));

        let parents: Vec<(TaskId, Option<TaskId>)> = drain(&mut rx)
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::TaskStarted { parent, .. } => Some((e.task_id, parent)),
                _ => None,
            })
            .collect();
        assert_eq!(
            parents,
            vec![
                (TaskId(0), None),
                (TaskId(1), Some(TaskId(0))),
                (TaskId(2), Some(TaskId(0))),
                (TaskId(3), None),
            ]
        );
    }

    /// A step reporter keeps its own channel handle, so output emitted after
    /// the task reporter is gone still arrives. This is what makes the
    /// spawned stdout/stderr readers in the core library safe.
    #[tokio::test]
    async fn step_reporter_outlives_its_task_reporter() {
        let (reporter, mut rx) = channel::<DemoTask>();
        let step = {
            let task = reporter.task(demo("a"));
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
        let (reporter, mut rx) = channel::<DemoTask>();
        let task = reporter.task(demo("a"));
        let step = task.step(1, 1, "run");

        drop(reporter);
        drop(task);
        assert!(rx.recv().await.is_some(), "buffered events still deliver");

        drop(step);
        while rx.recv().await.is_some() {}
        // Reaching here means recv() returned None rather than hanging.
    }

    /// The `--json` renderer planned in #493 depends on this wire format, so
    /// the tags and field names are pinned here rather than left to chance.
    /// The task payload's own shape is the caller's business and is asserted
    /// where that payload is defined.
    #[test]
    fn event_wire_format_is_stable() {
        let event = |kind| {
            serde_json::to_value(Event {
                task_id: TaskId(7),
                kind,
            })
            .expect("event should serialize")
        };

        assert_eq!(
            event(EventKind::TaskStarted {
                parent: None,
                task: demo("a"),
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_started",
                // A root task carries no `parent` key at all.
                // The payload nests under `task`; its inner shape is the
                // caller's to define.
                "task": { "kind": "demo", "name": "a" },
            })
        );
        assert_eq!(
            event(EventKind::TaskStarted {
                parent: Some(TaskId(3)),
                task: demo("a"),
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_started", "parent": 3,
                "task": { "kind": "demo", "name": "a" },
            })
        );
        assert_eq!(
            event(EventKind::<DemoTask>::StepStarted {
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
            event(EventKind::<DemoTask>::CommandStarted {
                command: "make build".to_owned(),
            }),
            serde_json::json!({
                "task_id": 7, "event": "command_started", "command": "make build",
            })
        );
        assert_eq!(
            event(EventKind::<DemoTask>::Output {
                stream: OutputStream::Stderr,
                line: "boom".to_owned(),
            }),
            serde_json::json!({
                "task_id": 7, "event": "output", "stream": "stderr", "line": "boom",
            })
        );
        assert_eq!(
            event(EventKind::<DemoTask>::Progress { position: 1024 }),
            serde_json::json!({ "task_id": 7, "event": "progress", "position": 1024 })
        );
        assert_eq!(
            event(EventKind::<DemoTask>::StepCompleted {
                outcome: StepOutcome::Failed,
            }),
            serde_json::json!({ "task_id": 7, "event": "step_completed", "outcome": "failed" })
        );

        // Task outcomes nest under `outcome`, internally tagged by `result`.
        assert_eq!(
            event(EventKind::<DemoTask>::TaskCompleted {
                outcome: TaskOutcome::succeeded(),
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": { "result": "succeeded" },
            })
        );
        assert_eq!(
            event(EventKind::<DemoTask>::TaskCompleted {
                outcome: TaskOutcome::Succeeded {
                    retained_output: vec!["kept".to_owned()],
                },
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": { "result": "succeeded", "retained_output": ["kept"] },
            })
        );
        assert_eq!(
            event(EventKind::<DemoTask>::TaskCompleted {
                outcome: TaskOutcome::Failed {
                    message: "no".to_owned(),
                    causes: vec!["because".to_owned()],
                },
            }),
            serde_json::json!({
                "task_id": 7, "event": "task_completed",
                "outcome": { "result": "failed", "message": "no", "causes": ["because"] },
            })
        );
        assert_eq!(
            event(EventKind::<DemoTask>::TaskCompleted {
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
