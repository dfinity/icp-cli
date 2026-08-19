//! Typed progress events passed from operations to the presentation layer.
//!
//! Operations (and the core library underneath them) emit [`Event`]s through
//! cheap-to-clone reporter handles ([`Reporter`] → [`TaskReporter`] →
//! [`StepReporter`]); the CLI's renderers consume the event stream and decide
//! how to display it. Events carry data, not prose — wording, layout, and
//! color are the renderer's job.
//!
//! Sends never block and never fail: the channel is unbounded, and with no
//! receiver events are simply dropped, so tests and headless callers get
//! silence for free. Errors do not travel on this stream — an operation's
//! `Result` remains the source of truth; [`TaskOutcome::Failed`] exists only
//! so a renderer can paint the failure state.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use serde::Serialize;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// Identifies one task (one canister-level unit of work) within an event
/// stream. Ids are assigned in task-creation order, so renderers can use them
/// to present tasks in a stable order regardless of completion order.
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
    TaskStarted { task: TaskKind },

    /// A step of the task began. Steps within a task are sequential;
    /// `number` is 1-based. `label` describes the step (it may span
    /// multiple lines).
    StepStarted {
        number: usize,
        total: usize,
        label: String,
    },

    /// One line of output produced while the task's current step runs.
    Output { stream: OutputStream, line: String },

    /// The task's current step finished.
    StepCompleted { outcome: StepOutcome },

    /// The task finished; no further events follow for this task.
    TaskCompleted { outcome: TaskOutcome },
}

/// What a task is doing, and to which canister.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskKind {
    Build { canister: String },
}

impl TaskKind {
    /// The canister this task operates on.
    pub fn canister(&self) -> &str {
        match self {
            TaskKind::Build { canister } => canister,
        }
    }
}

/// Where an output line came from.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
    /// A progress note from icp itself rather than a spawned tool.
    Info,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum TaskOutcome {
    Succeeded,
    /// `message` is for display only; the typed error stays on the
    /// operation's return path.
    Failed {
        message: String,
    },
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
    };
    (reporter, rx)
}

/// Entry point handed to an operation; spawns [`TaskReporter`]s.
#[derive(Debug, Clone)]
pub struct Reporter {
    inner: Option<ReporterInner>,
}

#[derive(Debug, Clone)]
struct ReporterInner {
    tx: UnboundedSender<Event>,
    next_task_id: Arc<AtomicU64>,
}

impl Reporter {
    /// A reporter whose events go nowhere.
    pub fn null() -> Self {
        Self { inner: None }
    }

    /// Begin a task, emitting [`EventKind::TaskStarted`].
    pub fn task(&self, task: TaskKind) -> TaskReporter {
        let Some(inner) = &self.inner else {
            return TaskReporter::null();
        };
        let task_id = TaskId(inner.next_task_id.fetch_add(1, Ordering::Relaxed));
        let _ = inner.tx.send(Event {
            task_id,
            kind: EventKind::TaskStarted { task },
        });
        TaskReporter {
            tx: Some(inner.tx.clone()),
            task_id,
        }
    }
}

/// Reports the lifecycle of one task.
#[derive(Debug, Clone)]
pub struct TaskReporter {
    tx: Option<UnboundedSender<Event>>,
    task_id: TaskId,
}

impl TaskReporter {
    /// A task reporter whose events go nowhere.
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

    /// Begin the task's next step, emitting [`EventKind::StepStarted`].
    /// `number` is 1-based.
    pub fn step(&self, number: usize, total: usize, label: impl Into<String>) -> StepReporter {
        self.send(EventKind::StepStarted {
            number,
            total,
            label: label.into(),
        });
        StepReporter {
            tx: self.tx.clone(),
            task_id: self.task_id,
        }
    }

    /// Finish the task, emitting [`EventKind::TaskCompleted`]. No further
    /// events should be sent for this task.
    pub fn finish(&self, outcome: TaskOutcome) {
        self.send(EventKind::TaskCompleted { outcome });
    }
}

/// Reports output produced during one step of a task.
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

    /// TEMPORARY: adapt a legacy line channel into a [`StepReporter`].
    ///
    /// Output lines emitted through the returned reporter are forwarded to
    /// `lines` by a background task, which drains and exits once the reporter
    /// and all of its clones are dropped. Used by call sites that still
    /// render through the old progress-bar channel (sync); remove once those
    /// paths take a real [`Reporter`].
    pub fn bridge_lines(lines: tokio::sync::mpsc::Sender<String>) -> Self {
        let (tx, mut rx) = unbounded_channel::<Event>();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let EventKind::Output { line, .. } = event.kind {
                    let _ = lines.send(line).await;
                }
            }
        });
        Self {
            tx: Some(tx),
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
