use serde::{Deserialize, Serialize};

/// Identifies a task within a single [`Reporter`](crate::Reporter)'s event stream.
///
/// Ids are only unique per reporter; two reporters both start counting at zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub u64);

/// The shape of a task, which tells a sink how to render it.
///
/// This is a closed enum on purpose: the event model is not semver-stable, so new
/// shapes are added here rather than smuggled through an open string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TaskKind {
    /// An open-ended activity whose duration is unknown, carrying only a message.
    Spinner,

    /// An activity made of discrete steps, each of which streams output lines.
    ///
    /// `output_label` names the kind of output the steps produce (for example
    /// `"Build"` or `"Sync"`) so a sink can label a replay of it.
    Steps { output_label: String },

    /// An activity that advances through a known number of bytes.
    Bytes { total: u64 },
}

/// How a task ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Outcome {
    /// The task did what it set out to do.
    Success,

    /// The task failed.
    Failure,

    /// The task ended without either succeeding or failing — it was skipped,
    /// superseded, or simply dropped.
    Neutral,
}

/// The severity of a [`Event::Notice`].
///
/// These mirror the user-facing `tracing` levels the CLI prints as product output;
/// they are not a logging facility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NoticeLevel {
    /// Plain user-facing output.
    Info,

    /// Something the user should be aware of but which does not stop the operation.
    Warn,

    /// Something that went wrong.
    Error,
}

/// A single observation emitted by an operation.
///
/// Events are the entire vocabulary an operation has for talking to the user. A
/// terminal renders them as progress bars, a test records them into a `Vec<Event>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Event {
    /// A task came into existence. Always the first event for a given `id`.
    TaskStarted {
        id: TaskId,
        kind: TaskKind,
        /// A short name for whatever the task acts on, usually a canister name.
        label: Option<String>,
    },

    /// The task's one-line status text changed.
    TaskMessage { id: TaskId, message: String },

    /// A [`TaskKind::Bytes`] task advanced to an absolute byte offset.
    TaskPosition { id: TaskId, position: u64 },

    /// The task ended. Always the last event for a given `id`.
    TaskFinished {
        id: TaskId,
        outcome: Outcome,
        message: Option<String>,
    },

    /// A step of a [`TaskKind::Steps`] task began. `index` is zero-based.
    StepStarted {
        id: TaskId,
        index: usize,
        title: String,
    },

    /// A line of output produced by the step currently in progress.
    StepOutput { id: TaskId, line: String },

    /// The step with this `index` finished.
    StepFinished { id: TaskId, index: usize },

    /// A user-facing message that does not belong to any task.
    Notice { level: NoticeLevel, message: String },
}

impl Event {
    /// The task this event belongs to, if any. [`Event::Notice`] belongs to none.
    pub fn task_id(&self) -> Option<TaskId> {
        match *self {
            Event::TaskStarted { id, .. }
            | Event::TaskMessage { id, .. }
            | Event::TaskPosition { id, .. }
            | Event::TaskFinished { id, .. }
            | Event::StepStarted { id, .. }
            | Event::StepOutput { id, .. }
            | Event::StepFinished { id, .. } => Some(id),
            Event::Notice { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_is_transparent_over_the_wire() {
        let json = serde_json::to_string(&TaskId(7)).unwrap();
        assert_eq!(json, "7");
        assert_eq!(serde_json::from_str::<TaskId>("7").unwrap(), TaskId(7));
    }

    #[test]
    fn events_round_trip_through_serde() {
        let events = vec![
            Event::TaskStarted {
                id: TaskId(0),
                kind: TaskKind::Spinner,
                label: Some("backend".into()),
            },
            Event::TaskMessage {
                id: TaskId(0),
                message: "Installing...".into(),
            },
            Event::TaskPosition {
                id: TaskId(0),
                position: 4096,
            },
            Event::StepStarted {
                id: TaskId(0),
                index: 0,
                title: "Building: step 1 of 2".into(),
            },
            Event::StepOutput {
                id: TaskId(0),
                line: "compiling".into(),
            },
            Event::StepFinished {
                id: TaskId(0),
                index: 0,
            },
            Event::TaskFinished {
                id: TaskId(0),
                outcome: Outcome::Success,
                message: Some("Installed successfully".into()),
            },
            Event::Notice {
                level: NoticeLevel::Warn,
                message: "not created yet".into(),
            },
        ];

        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), event);
        }
    }

    #[test]
    fn task_kind_carries_all_three_shapes_in_use_today() {
        // A plain spinner, a multi-step progress bar, and byte-position progress.
        assert_eq!(
            serde_json::to_value(TaskKind::Spinner).unwrap(),
            serde_json::json!({ "kind": "spinner" })
        );
        assert_eq!(
            serde_json::to_value(TaskKind::Steps {
                output_label: "Build".into()
            })
            .unwrap(),
            serde_json::json!({ "kind": "steps", "output_label": "Build" })
        );
        assert_eq!(
            serde_json::to_value(TaskKind::Bytes { total: 10 }).unwrap(),
            serde_json::json!({ "kind": "bytes", "total": 10 })
        );
    }

    #[test]
    fn only_notices_have_no_task_id() {
        assert_eq!(
            Event::TaskMessage {
                id: TaskId(3),
                message: "hi".into()
            }
            .task_id(),
            Some(TaskId(3))
        );
        assert_eq!(
            Event::Notice {
                level: NoticeLevel::Info,
                message: "hi".into()
            }
            .task_id(),
            None
        );
    }

    #[test]
    fn notice_levels_are_ordered_by_severity() {
        assert!(NoticeLevel::Info < NoticeLevel::Warn);
        assert!(NoticeLevel::Warn < NoticeLevel::Error);
    }
}
