use std::{
    fmt,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    cancel::CancelToken,
    event::{Event, NoticeLevel, Outcome, TaskId, TaskKind},
    output::{OutputWriter, RecordedStep, StepLog},
    sink::{DiscardSink, EventSink},
};

/// The handle an operation is given so it can report what it is doing.
///
/// A `Reporter` owns nothing terminal-shaped; it forwards [`Event`]s to an
/// [`EventSink`]. Clones share the sink, the id counter, and the cancel token.
#[derive(Clone)]
pub struct Reporter {
    sink: Arc<dyn EventSink>,
    next_id: Arc<AtomicU64>,
    cancel: CancelToken,
}

impl fmt::Debug for Reporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reporter")
            .field("sink", &self.sink)
            .field("next_id", &self.next_id.load(Ordering::SeqCst))
            .field("cancel", &self.cancel)
            .finish()
    }
}

impl Reporter {
    /// Report to `sink`, with a fresh cancel token.
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        Self::with_cancel_token(sink, CancelToken::new())
    }

    /// Report to `sink`, sharing an existing cancel token.
    pub fn with_cancel_token(sink: Arc<dyn EventSink>, cancel: CancelToken) -> Self {
        Self {
            sink,
            next_id: Arc::new(AtomicU64::new(0)),
            cancel,
        }
    }

    /// A reporter that throws everything away.
    pub fn discard() -> Self {
        Self::new(Arc::new(DiscardSink))
    }

    /// The cancellation signal shared with everything this reporter hands out.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// Send one event straight through to the sink.
    pub fn emit(&self, event: Event) {
        self.sink.emit(event);
    }

    /// Emit a user-facing message that does not belong to any task.
    pub fn notice(&self, level: NoticeLevel, message: impl Into<String>) {
        self.emit(Event::Notice {
            level,
            message: message.into(),
        });
    }

    /// Emit an [`NoticeLevel::Info`] notice.
    pub fn info(&self, message: impl Into<String>) {
        self.notice(NoticeLevel::Info, message);
    }

    /// Emit a [`NoticeLevel::Warn`] notice.
    pub fn warn(&self, message: impl Into<String>) {
        self.notice(NoticeLevel::Warn, message);
    }

    /// Emit a [`NoticeLevel::Error`] notice.
    pub fn error(&self, message: impl Into<String>) {
        self.notice(NoticeLevel::Error, message);
    }

    /// Start a task labelled with the thing it acts on, usually a canister name.
    pub fn task(&self, kind: TaskKind, label: impl Into<String>) -> Task {
        self.start(kind, Some(label.into()))
    }

    /// Start a task that is not about any one named thing.
    pub fn unlabelled_task(&self, kind: TaskKind) -> Task {
        self.start(kind, None)
    }

    fn start(&self, kind: TaskKind, label: Option<String>) -> Task {
        let id = TaskId(self.next_id.fetch_add(1, Ordering::SeqCst));
        self.emit(Event::TaskStarted { id, kind, label });

        Task {
            id,
            reporter: self.clone(),
            next_step: 0,
            open_step: None,
            finished: false,
            log: Arc::new(StepLog::default()),
        }
    }
}

/// One unit of reportable work.
///
/// A task always ends: finish it explicitly with [`succeed`](Task::succeed),
/// [`fail`](Task::fail), or [`skip`](Task::skip), or let it drop and it reports
/// [`Outcome::Neutral`]. That guarantee is what lets a sink close out a live
/// progress bar even on an early return.
#[derive(Debug)]
pub struct Task {
    id: TaskId,
    reporter: Reporter,
    next_step: usize,
    open_step: Option<usize>,
    finished: bool,
    log: Arc<StepLog>,
}

impl Task {
    /// This task's id, as it appears in the event stream.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// The reporter this task belongs to.
    pub fn reporter(&self) -> &Reporter {
        &self.reporter
    }

    /// Replace the task's one-line status text.
    pub fn message(&self, message: impl Into<String>) {
        self.reporter.emit(Event::TaskMessage {
            id: self.id,
            message: message.into(),
        });
    }

    /// Report an absolute byte offset for a [`TaskKind::Bytes`] task.
    pub fn position(&self, position: u64) {
        self.reporter.emit(Event::TaskPosition {
            id: self.id,
            position,
        });
    }

    /// Begin a step of a [`TaskKind::Steps`] task.
    ///
    /// # Panics
    ///
    /// Panics if a step is already in progress.
    pub fn begin_step(&mut self, title: impl Into<String>) {
        assert!(self.open_step.is_none(), "step already in progress");

        let index = self.next_step;
        self.next_step += 1;
        self.open_step = Some(index);

        let title = title.into();
        self.log.begin(title.clone());

        self.reporter.emit(Event::StepStarted {
            id: self.id,
            index,
            title,
        });
    }

    /// Report a line of output from the step in progress.
    pub fn step_output(&self, line: impl Into<String>) {
        self.output().line(line);
    }

    /// A handle for reporting the output of the step in progress.
    ///
    /// This is what gets handed to whatever actually produces the output — a
    /// subprocess reader, a plugin runtime — so it can report lines without
    /// depending on this task, or on anything that renders it.
    pub fn output(&self) -> OutputWriter {
        OutputWriter::new(self.reporter.clone(), self.id, self.log.clone())
    }

    /// Every step of this task, with the output it produced.
    ///
    /// Progress is transient, so an operation that has to explain a failure after
    /// the fact reads the step back from here.
    pub fn recorded_steps(&self) -> Vec<RecordedStep> {
        self.log.recorded()
    }

    /// End the step in progress.
    ///
    /// # Panics
    ///
    /// Panics if no step is in progress.
    pub fn end_step(&mut self) {
        let index = self.open_step.take().expect("no step in progress");
        self.reporter
            .emit(Event::StepFinished { id: self.id, index });
    }

    /// Finish the task successfully.
    pub fn succeed(mut self, message: impl Into<String>) {
        self.complete(Outcome::Success, Some(message.into()));
    }

    /// Finish the task as failed.
    pub fn fail(mut self, message: impl Into<String>) {
        self.complete(Outcome::Failure, Some(message.into()));
    }

    /// Finish the task without a verdict — skipped, superseded, or not applicable.
    pub fn skip(mut self, message: impl Into<String>) {
        self.complete(Outcome::Neutral, Some(message.into()));
    }

    /// Await `future`, then finish the task from its result.
    ///
    /// This is the shape shared by every "do one thing per canister and report how
    /// it went" operation.
    pub async fn run<F, T, E>(
        self,
        future: F,
        success_message: impl FnOnce() -> String,
        error_message: impl FnOnce(&E) -> String,
    ) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        self.run_with(future, success_message, error_message, |_| false)
            .await
    }

    /// [`run`](Task::run), but errors matching `is_success_error` are reported as
    /// successes while still being returned to the caller.
    pub async fn run_with<F, T, E>(
        mut self,
        future: F,
        success_message: impl FnOnce() -> String,
        error_message: impl FnOnce(&E) -> String,
        is_success_error: impl FnOnce(&E) -> bool,
    ) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        let result = future.await;

        let (outcome, message) = match &result {
            Ok(_) => (Outcome::Success, success_message()),
            Err(err) if is_success_error(err) => (Outcome::Success, error_message(err)),
            Err(err) => (Outcome::Failure, error_message(err)),
        };
        self.complete(outcome, Some(message));

        result
    }

    fn complete(&mut self, outcome: Outcome, message: Option<String>) {
        if self.finished {
            return;
        }
        self.finished = true;

        if self.open_step.is_some() {
            self.end_step();
        }

        self.reporter.emit(Event::TaskFinished {
            id: self.id,
            outcome,
            message,
        });
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        // A dropped task still has to close out, or a sink is left holding a bar that
        // spins forever.
        self.complete(Outcome::Neutral, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::RecordingSink;
    use futures::executor::block_on;

    fn recorder() -> (Reporter, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink::new());
        (Reporter::new(sink.clone()), sink)
    }

    #[test]
    fn a_spinner_task_reports_start_message_and_outcome() {
        let (reporter, sink) = recorder();

        let task = reporter.task(TaskKind::Spinner, "backend");
        task.message("Installing...");
        task.succeed("Installed successfully");

        assert_eq!(
            sink.events(),
            vec![
                Event::TaskStarted {
                    id: TaskId(0),
                    kind: TaskKind::Spinner,
                    label: Some("backend".into()),
                },
                Event::TaskMessage {
                    id: TaskId(0),
                    message: "Installing...".into(),
                },
                Event::TaskFinished {
                    id: TaskId(0),
                    outcome: Outcome::Success,
                    message: Some("Installed successfully".into()),
                },
            ]
        );
    }

    #[test]
    fn task_ids_are_handed_out_in_creation_order() {
        let (reporter, sink) = recorder();

        let _a = reporter.task(TaskKind::Spinner, "a");
        let _b = reporter.task(TaskKind::Spinner, "b");

        let ids: Vec<_> = sink.events().iter().filter_map(Event::task_id).collect();
        assert_eq!(ids, vec![TaskId(0), TaskId(1)]);
    }

    #[test]
    fn dropping_a_task_finishes_it_neutrally() {
        let (reporter, sink) = recorder();

        drop(reporter.task(TaskKind::Spinner, "abandoned"));

        assert_eq!(
            sink.events().last().unwrap(),
            &Event::TaskFinished {
                id: TaskId(0),
                outcome: Outcome::Neutral,
                message: None,
            }
        );
    }

    #[test]
    fn a_task_finishes_exactly_once() {
        let (reporter, sink) = recorder();

        reporter.task(TaskKind::Spinner, "once").skip("Skipped");

        let finishes = sink
            .events()
            .iter()
            .filter(|e| matches!(e, Event::TaskFinished { .. }))
            .count();
        assert_eq!(finishes, 1);
    }

    #[test]
    fn steps_are_numbered_and_carry_their_output() {
        let (reporter, sink) = recorder();

        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: "Build".into(),
            },
            "backend",
        );
        task.begin_step("Building: step 1 of 2 cargo build");
        task.step_output("compiling");
        task.end_step();
        task.begin_step("Building: step 2 of 2 wasm-opt");
        task.end_step();
        task.succeed("Built successfully");

        assert_eq!(
            sink.events()[1..],
            [
                Event::StepStarted {
                    id: TaskId(0),
                    index: 0,
                    title: "Building: step 1 of 2 cargo build".into(),
                },
                Event::StepOutput {
                    id: TaskId(0),
                    line: "compiling".into(),
                },
                Event::StepFinished {
                    id: TaskId(0),
                    index: 0,
                },
                Event::StepStarted {
                    id: TaskId(0),
                    index: 1,
                    title: "Building: step 2 of 2 wasm-opt".into(),
                },
                Event::StepFinished {
                    id: TaskId(0),
                    index: 1,
                },
                Event::TaskFinished {
                    id: TaskId(0),
                    outcome: Outcome::Success,
                    message: Some("Built successfully".into()),
                },
            ]
        );
    }

    /// The rolling view a sink draws is transient, so a task also keeps its steps'
    /// output for an operation that has to print the failing step afterwards.
    #[test]
    fn a_task_keeps_the_output_of_every_step() {
        let (reporter, _sink) = recorder();

        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: "Build".into(),
            },
            "backend",
        );
        task.begin_step("step 1");
        task.step_output("compiling");
        task.end_step();
        task.begin_step("step 2");
        task.output().line("optimizing");
        task.end_step();

        assert_eq!(
            task.recorded_steps(),
            vec![
                crate::RecordedStep {
                    title: "step 1".into(),
                    lines: vec!["compiling".into()],
                },
                crate::RecordedStep {
                    title: "step 2".into(),
                    lines: vec!["optimizing".into()],
                },
            ]
        );
    }

    /// A command has as many output streams as it has streams to read, so the
    /// writer is handed out by clone.
    #[test]
    fn every_clone_of_a_writer_reports_to_the_same_task() {
        let (reporter, sink) = recorder();

        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: "Build".into(),
            },
            "backend",
        );
        task.begin_step("step 1");

        let stdout = task.output();
        let stderr = stdout.clone();
        stdout.line("out");
        stderr.line("err");

        let lines: Vec<String> = sink
            .events()
            .into_iter()
            .filter_map(|event| match event {
                Event::StepOutput { id, line } if id == task.id() => Some(line),
                _ => None,
            })
            .collect();
        assert_eq!(lines, vec!["out".to_owned(), "err".to_owned()]);
    }

    /// The writer outlives the borrow of the task, since it is handed to code that
    /// keeps reporting while the operation moves on.
    #[test]
    fn a_writer_can_be_sent_to_another_thread() {
        let (reporter, sink) = recorder();

        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: "Build".into(),
            },
            "backend",
        );
        task.begin_step("step 1");

        let writer = task.output();
        std::thread::spawn(move || writer.line("from a thread"))
            .join()
            .unwrap();

        assert!(sink.events().contains(&Event::StepOutput {
            id: task.id(),
            line: "from a thread".into(),
        }));
    }

    #[test]
    fn finishing_closes_a_step_left_open() {
        let (reporter, sink) = recorder();

        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: "Build".into(),
            },
            "backend",
        );
        task.begin_step("Building: step 1 of 1");
        task.fail("Failed to build canister: boom");

        assert_eq!(
            sink.events()[2..],
            [
                Event::StepFinished {
                    id: TaskId(0),
                    index: 0,
                },
                Event::TaskFinished {
                    id: TaskId(0),
                    outcome: Outcome::Failure,
                    message: Some("Failed to build canister: boom".into()),
                },
            ]
        );
    }

    #[test]
    #[should_panic(expected = "step already in progress")]
    fn steps_may_not_overlap() {
        let (reporter, _sink) = recorder();

        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: "Build".into(),
            },
            "backend",
        );
        task.begin_step("one");
        task.begin_step("two");
    }

    #[test]
    fn byte_tasks_report_absolute_positions() {
        let (reporter, sink) = recorder();

        let task = reporter.unlabelled_task(TaskKind::Bytes { total: 100 });
        task.position(0);
        task.position(64);
        drop(task);

        assert_eq!(
            sink.events()[..3],
            [
                Event::TaskStarted {
                    id: TaskId(0),
                    kind: TaskKind::Bytes { total: 100 },
                    label: None,
                },
                Event::TaskPosition {
                    id: TaskId(0),
                    position: 0,
                },
                Event::TaskPosition {
                    id: TaskId(0),
                    position: 64,
                },
            ]
        );
    }

    #[test]
    fn run_reports_success_and_returns_the_value() {
        let (reporter, sink) = recorder();

        let result: Result<u8, String> = block_on(reporter.task(TaskKind::Spinner, "ok").run(
            async { Ok(7) },
            || "Compatible".to_string(),
            |e: &String| format!("Incompatible: {e}"),
        ));

        assert_eq!(result, Ok(7));
        assert_eq!(
            sink.events().last().unwrap(),
            &Event::TaskFinished {
                id: TaskId(0),
                outcome: Outcome::Success,
                message: Some("Compatible".into()),
            }
        );
    }

    #[test]
    fn run_reports_failure_and_returns_the_error() {
        let (reporter, sink) = recorder();

        let result: Result<u8, String> = block_on(reporter.task(TaskKind::Spinner, "bad").run(
            async { Err("boom".to_string()) },
            || "Compatible".to_string(),
            |e: &String| format!("Incompatible: {e}"),
        ));

        assert_eq!(result, Err("boom".to_string()));
        assert_eq!(
            sink.events().last().unwrap(),
            &Event::TaskFinished {
                id: TaskId(0),
                outcome: Outcome::Failure,
                message: Some("Incompatible: boom".into()),
            }
        );
    }

    #[test]
    fn run_with_can_report_an_error_as_a_success() {
        let (reporter, sink) = recorder();

        let result: Result<u8, String> =
            block_on(reporter.task(TaskKind::Spinner, "soft").run_with(
                async { Err("already done".to_string()) },
                || "Done".to_string(),
                |e: &String| format!("Nothing to do: {e}"),
                |e: &String| e == "already done",
            ));

        assert!(result.is_err());
        assert_eq!(
            sink.events().last().unwrap(),
            &Event::TaskFinished {
                id: TaskId(0),
                outcome: Outcome::Success,
                message: Some("Nothing to do: already done".into()),
            }
        );
    }

    #[test]
    fn notices_carry_their_level_and_no_task() {
        let (reporter, sink) = recorder();

        reporter.info("Installing canisters:");
        reporter.warn("not created yet");
        reporter.error("it broke");

        assert_eq!(
            sink.events(),
            vec![
                Event::Notice {
                    level: NoticeLevel::Info,
                    message: "Installing canisters:".into(),
                },
                Event::Notice {
                    level: NoticeLevel::Warn,
                    message: "not created yet".into(),
                },
                Event::Notice {
                    level: NoticeLevel::Error,
                    message: "it broke".into(),
                },
            ]
        );
    }

    #[test]
    fn a_discarding_reporter_still_works() {
        let reporter = Reporter::discard();
        reporter.task(TaskKind::Spinner, "quiet").succeed("done");
        reporter.info("also quiet");
    }

    #[test]
    fn clones_share_the_cancel_token() {
        let (reporter, _sink) = recorder();
        let clone = reporter.clone();

        clone.cancel_token().cancel();
        assert!(reporter.cancel_token().is_cancelled());
    }
}
