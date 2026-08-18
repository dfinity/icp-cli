//! Progress and user-facing notices, expressed as data.
//!
//! Operations in `icp-cli` used to talk to the terminal directly through
//! `indicatif`, which welded them to the binary. This crate inverts that: an
//! operation is handed a [`Reporter`], reports what it is doing as [`Event`]s, and
//! never learns whether anything is rendering them. The CLI attaches an
//! [`EventSink`] that draws progress bars; a test attaches a [`RecordingSink`] and
//! asserts on a `Vec<Event>`.
//!
//! The model carries the three progress shapes the CLI uses today — a plain
//! spinner, a multi-step bar that streams command output, and byte-position
//! progress — plus [`Event::Notice`], which represents the user-facing `info!` /
//! `warn!` / `error!` calls that this CLI prints as product output rather than as
//! logging.
//!
//! Code that produces the output of a step — a subprocess reader, a plugin runtime —
//! is handed an [`OutputWriter`] rather than a channel. Each line it reports becomes
//! an [`Event::StepOutput`] and is also kept in the task's step log, so an operation
//! that fails can replay the whole failing step once the progress it drew is gone.
//!
//! # Stability
//!
//! The event model is **not** semver-stable. It ships at `0.x`, moves in lockstep
//! with `icp-cli`, and is never published. Enums are `#[non_exhaustive]` so
//! variants can be added without ceremony; [`TaskKind`] in particular is a closed
//! enum, not an open string, because nothing outside this workspace consumes it.
//!
//! The event stream does not drive `--json`. `--json` means the command's final
//! result, and progress never appears in it.
//!
//! # Example
//!
//! ```
//! use std::sync::Arc;
//! use icp_events::{Event, Outcome, RecordingSink, Reporter, TaskId, TaskKind};
//!
//! let sink = Arc::new(RecordingSink::new());
//! let reporter = Reporter::new(sink.clone());
//!
//! let task = reporter.task(TaskKind::Spinner, "backend");
//! task.message("Installing...");
//! task.succeed("Installed successfully");
//!
//! assert_eq!(
//!     sink.events().last().unwrap(),
//!     &Event::TaskFinished {
//!         id: TaskId(0),
//!         outcome: Outcome::Success,
//!         message: Some("Installed successfully".into()),
//!     },
//! );
//! ```

mod cancel;
mod event;
mod output;
mod reporter;
mod sink;

pub use cancel::{CancelToken, Cancelled};
pub use event::{Event, NoticeLevel, Outcome, TaskId, TaskKind};
pub use output::{MAX_RECORDED_LINES_PER_STEP, OutputWriter, RecordedStep};
pub use reporter::{Reporter, Task};
pub use sink::{DiscardSink, EventSink, RecordingSink};
