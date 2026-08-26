//! Presentation layer for [`icp_events`] streams.
//!
//! Operations emit events through a [`Reporter`]; a [`Renderer`] consumes the
//! stream and owns everything user-facing. Commands pick a renderer with
//! [`Renderer::for_ctx`] and drive it with [`Renderer::run`] alongside the
//! operation.
//!
//! The event stream carries no operation vocabulary — a task announces itself
//! with a title, an optional subject, and the shape of its widget. Everything
//! below composes display text from those three facts and nothing else, so a
//! new kind of work needs no change here at all. The chrome (brackets,
//! dashes, indentation, tick marks) is the renderer's; the nouns are the
//! operation's.

use std::collections::{BTreeMap, VecDeque};

use icp_events::{
    Event, Failure, FailureReport, Reporter, Shape, Task, TaskId, TaskOutcome, TaskReporter,
};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::error;

mod interactive;
mod plain;
mod spinner;
mod style;

pub(crate) use interactive::InteractiveRenderer;
pub(crate) use plain::PlainRenderer;
pub(crate) use spinner::{ProgressManager, ProgressManagerSettings};

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

/// Run an operation with a fresh event channel and a renderer driving its
/// display: the reporter is handed to `op`, and once `op` finishes the stream
/// is closed and the renderer flushes (failure dumps) before the operation's
/// result is returned.
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
            // The command's returned error reports this; a dump would only
            // say it twice.
            Err(error) => task.finish(TaskOutcome::failed_silently(error.to_string())),
        }

        result
    })
    .await
}

/// What a task said about itself, plus where it sits in the task tree. Every
/// piece of display text is composed from these.
pub(super) struct TaskInfo {
    subject: Option<String>,
    title: String,
    shape: Shape,
    /// Levels of nesting below the root, for indentation.
    depth: usize,
}

impl TaskInfo {
    /// Prefix each line of captured or retained output so concurrent tasks
    /// stay attributable. Tasks with no subject contribute no prefix.
    fn line(&self, text: &str) -> String {
        match &self.subject {
            Some(subject) => format!("[{subject}] {text}"),
            None => text.to_owned(),
        }
    }

    /// Label for the task's live widget. A counter's bar is too narrow to
    /// carry both, so it is labelled by what it is transferring; everything
    /// else is labelled by what it is working on.
    fn widget_prefix(&self) -> String {
        let indent = INDENT.repeat(self.depth);
        match (&self.shape, &self.subject) {
            (Shape::Counter { .. }, _) | (_, None) => format!("{indent}{}", self.title),
            (_, Some(subject)) => format!("{indent}[{subject}]"),
        }
    }

    /// Message shown while the task runs, before any step reports in.
    fn running_message(&self) -> String {
        format!("{}...", self.title)
    }

    /// Live header shown while a step runs. `label` may span multiple lines.
    fn step_header(&self, number: usize, total: usize, label: &str) -> String {
        format!("{}: step {number} of {total} {label}", self.title)
    }

    /// Final widget message on failure.
    fn failure_message(&self, message: &str) -> String {
        format!("{} failed: {message}", self.title)
    }

    /// First line of the task's deferred failure dump.
    fn failure_header(&self) -> String {
        match &self.subject {
            Some(subject) => format!("----- {} failed: '{subject}' -----", self.title),
            None => format!("----- {} failed -----", self.title),
        }
    }
}

/// Captured output of one task, kept so a failure can be replayed after the
/// live view is gone.
pub(super) struct TaskLog {
    info: TaskInfo,
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
    fn new(info: TaskInfo) -> Self {
        Self {
            info,
            finished_steps: Vec::new(),
            current_step: None,
            failure: None,
        }
    }

    fn info(&self) -> &TaskInfo {
        &self.info
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

    fn fail(&mut self, failure: Failure) {
        self.failure = Some(failure);
    }

    /// Render the captured output. When `all_steps` is true, output from
    /// every step is included; otherwise only the last (failing) step is
    /// shown. Tasks that never reported a step have nothing to replay.
    fn dump(&self, all_steps: bool) -> Vec<String> {
        if self.finished_steps.is_empty() && self.current_step.is_none() {
            return Vec::new();
        }

        let info = &self.info;
        let mut lines = vec![info.line(&format!("{} output:", info.title))];

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
                    lines.push(info.line(&format!("{line}:")));
                }
            }

            if step.lines.is_empty() {
                lines.push(info.line("<no output>"));
            } else {
                lines.extend(
                    step.lines
                        .iter()
                        .map(|line| info.line(&format!("> {line}"))),
                );
            }
        }

        lines
    }
}

/// Print output lines a task retained past its rolling step view (e.g.
/// sync-plugin stderr), attributed to the task that produced them.
fn retained_lines(info: &TaskInfo, lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| info.line(line)).collect()
}

/// Print the failure dump for every failed task, in task-creation order,
/// followed by any epilogues the failures asked for (each printed once).
fn dump_failures(logs: &BTreeMap<TaskId, TaskLog>, all_steps: bool) {
    let mut epilogues: Vec<&str> = Vec::new();

    for log in logs.values() {
        let Some(failure) = &log.failure else {
            continue;
        };
        let body = match &failure.report {
            // The command's returned error already carries this one.
            FailureReport::Silent => continue,
            FailureReport::Summary => {
                let mut body = vec![format!("'{}'", failure.message)];
                body.extend(
                    failure
                        .causes
                        .iter()
                        .map(|cause| format!("  caused by: {cause}")),
                );
                body
            }
            FailureReport::Detail { lines } => lines.clone(),
        };

        error!("{}", log.info().failure_header());
        for line in body {
            error!("{line}");
        }
        for line in log.dump(all_steps) {
            error!("{line}");
        }

        if let Some(epilogue) = failure.epilogue.as_deref()
            && !epilogues.contains(&epilogue)
        {
            epilogues.push(epilogue);
        }
    }

    for epilogue in epilogues {
        error!("{epilogue}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(title: &str, subject: Option<&str>, depth: usize) -> TaskInfo {
        TaskInfo {
            subject: subject.map(str::to_owned),
            title: title.to_owned(),
            shape: Shape::Spinner,
            depth,
        }
    }

    /// Every line of display text is composed from title, subject and shape.
    /// These are the compositions, spelled out so a change to any of them is
    /// a deliberate one.
    #[test]
    fn display_text_is_composed_from_title_and_subject() {
        let build = info("Building", Some("frontend"), 0);

        assert_eq!(build.widget_prefix(), "[frontend]");
        assert_eq!(build.running_message(), "Building...");
        assert_eq!(
            build.step_header(1, 3, "(script)"),
            "Building: step 1 of 3 (script)"
        );
        assert_eq!(build.line("> hello"), "[frontend] > hello");
        assert_eq!(build.failure_message("exit 1"), "Building failed: exit 1");
        assert_eq!(
            build.failure_header(),
            "----- Building failed: 'frontend' -----"
        );
    }

    /// A task with no subject contributes no `[name]` prefix, and is labelled
    /// by what it is doing instead.
    #[test]
    fn a_subjectless_task_is_labelled_by_its_title() {
        let bundle = info("Bundling", None, 0);

        assert_eq!(bundle.widget_prefix(), "Bundling");
        assert_eq!(bundle.line("> hello"), "> hello");
        assert_eq!(bundle.failure_header(), "----- Bundling failed -----");
    }

    /// A counter's bar has no room for both, so it is labelled by what is
    /// moving rather than by the canister it belongs to.
    #[test]
    fn a_counter_is_labelled_by_its_title_even_with_a_subject() {
        let transfer = TaskInfo {
            shape: Shape::Counter { total: 4096 },
            ..info("WASM module", Some("frontend"), 0)
        };

        assert_eq!(transfer.widget_prefix(), "WASM module");
        // Output still attributes to the canister.
        assert_eq!(transfer.line("note"), "[frontend] note");
    }

    #[test]
    fn nesting_indents_the_widget_label() {
        assert_eq!(
            info("Installing", Some("frontend"), 1).widget_prefix(),
            "  [frontend]"
        );
        assert_eq!(info("Deploying", None, 2).widget_prefix(), "    Deploying");
    }

    #[test]
    fn a_dump_reproduces_the_captured_step_output() {
        let mut log = TaskLog::new(info("Building", Some("my-canister"), 0));
        log.start_step("Building: step 1 of 2 (script)".to_owned());
        log.push_line("hidden".to_owned());
        log.end_step();
        log.start_step("Building: step 2 of 2 (script)".to_owned());
        log.push_line("boom".to_owned());
        log.end_step();

        assert_eq!(
            log.dump(false),
            vec![
                "[my-canister] Building output:",
                "[my-canister] Building: step 2 of 2 (script):",
                "[my-canister] > boom",
            ]
        );
        assert_eq!(
            log.dump(true),
            vec![
                "[my-canister] Building output:",
                "[my-canister] Building: step 1 of 2 (script):",
                "[my-canister] > hidden",
                "[my-canister] Building: step 2 of 2 (script):",
                "[my-canister] > boom",
            ],
            "every step is replayed under --debug"
        );
    }

    /// A step that produced nothing still says so, rather than rendering as a
    /// bare header.
    #[test]
    fn a_silent_step_is_reported_as_such() {
        let mut log = TaskLog::new(info("Syncing", Some("frontend"), 0));
        log.start_step("Syncing: step 1 of 1 (assets)".to_owned());
        log.end_step();

        assert_eq!(
            log.dump(false),
            vec![
                "[frontend] Syncing output:",
                "[frontend] Syncing: step 1 of 1 (assets):",
                "[frontend] <no output>",
            ]
        );
    }

    /// Tasks that never reported a step have nothing to replay.
    #[test]
    fn a_stepless_task_dumps_nothing() {
        let log = TaskLog::new(info("Installing", Some("frontend"), 0));
        assert!(log.dump(true).is_empty());
    }

    #[test]
    fn retained_output_is_attributed_to_its_task() {
        let info = info("Syncing", Some("frontend"), 0);
        assert_eq!(
            retained_lines(&info, &["one".to_owned(), "two".to_owned()]),
            vec!["[frontend] one", "[frontend] two"]
        );
    }
}
