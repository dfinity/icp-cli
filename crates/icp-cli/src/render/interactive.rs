//! Live progress-bar renderer: one indicatif widget per task, a rolling
//! window of the current step's output beneath it, and a ✔/✘ finish state.
//! Nested tasks indent under their parent, and phases print as plain headings
//! above the bars. Failed tasks replay their captured output once the stream
//! ends.

use std::{collections::BTreeMap, time::Duration};

use icp_events::{EventKind, TaskId, TaskOutcome};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use tracing::{debug, info};

use super::style::{
    COLOR_FAILURE, COLOR_REGULAR, COLOR_SUCCESS, TICK_EMPTY, TICK_FAILURE, TICK_SUCCESS, make_style,
};
use icp_project::operations::task::{Event, Widget};

use super::{INDENT, RollingLines, TaskLog, dump_failures};

/// Number of output lines shown live under a task's progress bar.
const LIVE_WINDOW_LINES: usize = 4;

pub(crate) struct InteractiveRenderer {
    screen: Box<dyn Screen>,
    multi_progress: MultiProgress,
    tasks: BTreeMap<TaskId, TaskView>,
    /// Whether the live view holds bars it has drawn. On a terminal indicatif
    /// leaves the cursor at the end of the last one, so whatever is printed
    /// once the view is let go of would run on from it.
    drawn: bool,
}

/// Where the live view is drawn, and its last line ended.
trait Screen: Send {
    /// A fresh live view.
    fn view(&self) -> MultiProgress;
    /// End the line a view left the cursor on.
    fn end_line(&self);
}

/// The process's stderr, where nothing is drawn unless it is a terminal.
struct Stderr;

impl Screen for Stderr {
    fn view(&self) -> MultiProgress {
        MultiProgress::new()
    }

    fn end_line(&self) {
        eprintln!();
    }
}

struct TaskView {
    log: TaskLog,
    /// The live widget, or `None` for a phase — a heading has no live state,
    /// so it is printed once above the bars rather than animated.
    bar: Option<ProgressBar>,
    /// Header of the step currently running, shown above the live window.
    /// While a script command runs this is the step headline plus that
    /// command, so output is attributed to the command producing it.
    header: String,
    /// First line of the current step's full header, used to rebuild the
    /// live header when a command starts.
    headline: String,
    /// Rolling window over the current step's most recent output lines.
    window: RollingLines,
}

impl InteractiveRenderer {
    pub(crate) fn new() -> Self {
        Self::on(Box::new(Stderr))
    }

    fn on(screen: Box<dyn Screen>) -> Self {
        Self {
            multi_progress: screen.view(),
            screen,
            tasks: BTreeMap::new(),
            drawn: false,
        }
    }

    /// How deeply a task nests, from its parent's depth. A task whose parent
    /// is unknown is treated as top-level.
    fn depth_of(&self, parent: Option<TaskId>) -> usize {
        parent
            .and_then(|id| self.tasks.get(&id))
            .map(|view| view.log.depth() + 1)
            .unwrap_or(0)
    }

    /// Announce a heading.
    ///
    /// A top-level heading closes the live view first: dropping the
    /// `MultiProgress` leaves its last frame on the terminal, so the bars
    /// above become scrollback and the heading lands beneath them, with the
    /// next phase's bars drawing below that. Without this the finished bars
    /// would be redrawn *under* each new heading and the run would read out
    /// of order. Nested headings have live siblings to preserve, so they are
    /// written through the live view instead.
    ///
    /// The heading goes out over tracing rather than as a bar so it survives
    /// a hidden draw target — under a pipe or in CI there are no bars, but
    /// the phase headings still belong in the log.
    fn heading(&mut self, depth: usize, title: &str) {
        let line = format!("{}{title}", INDENT.repeat(depth));
        if depth == 0 {
            self.release();
            info!("{line}");
        } else {
            self.multi_progress.suspend(|| info!("{line}"));
        }
    }

    pub(crate) fn handle(&mut self, event: Event) {
        match event.kind {
            EventKind::TaskStarted { parent, task } => {
                let depth = self.depth_of(parent);
                let indent = INDENT.repeat(depth);
                let presentation = task.presentation();
                // Bars are configured fully before insertion: adding to the
                // MultiProgress switches the draw target, and `set_style`
                // adapts a style to stderr only while the bar is still
                // detached.
                let bar = match presentation.widget() {
                    // A heading has no live state; it just titles whatever
                    // nests beneath it.
                    Widget::Heading { title } => {
                        self.heading(depth, &title);
                        None
                    }
                    // Quantifiable work gets a determinate byte bar, labeled
                    // by the blob rather than the canister.
                    Widget::Bytes { label, total } => Some(
                        self.multi_progress.add(
                            ProgressBar::new(total)
                                .with_style(transfer_style())
                                .with_prefix(format!("{indent}{label}")),
                        ),
                    ),
                    // The bracket decoration is this renderer's convention,
                    // matching the captured-output and failure-dump prefixes.
                    Widget::Indeterminate { label } => {
                        let mut bar = ProgressBar::new_spinner()
                            .with_style(make_style(TICK_EMPTY, COLOR_REGULAR))
                            .with_prefix(format!("{indent}[{label}]"));
                        if let Some(message) = presentation.running_message() {
                            bar = bar.with_message(message);
                        }
                        let bar = self.multi_progress.add(bar);
                        bar.enable_steady_tick(Duration::from_millis(120));
                        Some(bar)
                    }
                };
                self.drawn |= bar.is_some();

                self.tasks.insert(
                    event.task_id,
                    TaskView {
                        log: TaskLog::new(task, depth),
                        bar,
                        header: String::new(),
                        headline: String::new(),
                        window: RollingLines::new(LIVE_WINDOW_LINES),
                    },
                );
            }

            EventKind::Progress { position } => {
                if let Some(view) = self.tasks.get(&event.task_id)
                    && let Some(bar) = &view.bar
                {
                    bar.set_position(position);
                }
            }

            EventKind::StepStarted {
                number,
                total,
                label,
            } => {
                let Some(view) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };
                view.header = view.log.presentation().step_header(number, total, &label);
                view.headline = view
                    .header
                    .lines()
                    .find(|line| !line.is_empty())
                    .unwrap_or_default()
                    .to_owned();
                view.window = RollingLines::new(LIVE_WINDOW_LINES);
                view.log.start_step(view.header.clone());
                // The bar is deliberately not updated here: the header shows
                // once the step's first output line arrives (the Output
                // branch), so silent steps draw nothing.
            }

            EventKind::CommandStarted { command } => {
                let Some(view) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };
                // Show only the running command under the step headline, and
                // reset the live window so a previous command's output isn't
                // attributed to this one. The captured log is unaffected.
                view.header = format!("{}\n$ {command}", view.headline);
                view.window = RollingLines::new(LIVE_WINDOW_LINES);
                if let Some(bar) = &view.bar {
                    bar.set_message(view.header.clone());
                }
            }

            EventKind::Output { line, .. } => {
                let Some(view) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };

                // A heading drives no widget and reports no output; there
                // is no live window to roll.
                let Some(bar) = &view.bar else {
                    return;
                };

                debug!("{}", view.log.line(&line));

                view.window.push(line.clone());
                view.log.push_line(line);

                // Update progress-bar with rolling terminal output
                // Make the output
                // │ look prettier...
                // └
                let rolled = view.window.iter().map(|s| format!("│ {s}")).join("\n");
                bar.set_message(format!("{}\n{rolled}\n└\n\n", view.header));
            }

            EventKind::StepCompleted { .. } => {
                if let Some(view) = self.tasks.get_mut(&event.task_id) {
                    view.log.end_step();
                }
            }

            EventKind::TaskCompleted { outcome } => {
                let Some(view) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };

                match outcome {
                    TaskOutcome::Succeeded { retained_output } => {
                        if let Some(bar) = &view.bar {
                            // A byte bar has no message or tick slot, so it
                            // just freezes at its final position.
                            if let Some(message) = view.log.presentation().success_message() {
                                bar.set_style(make_style(TICK_SUCCESS, COLOR_SUCCESS));
                                bar.set_message(message);
                            }
                            bar.finish();
                        }
                        super::print_retained(view.log.task(), &retained_output);
                    }
                    TaskOutcome::Failed { message, causes } => {
                        if let Some(bar) = &view.bar {
                            if let Some(rendered) =
                                view.log.presentation().failure_message(&message)
                            {
                                bar.set_style(make_style(TICK_FAILURE, COLOR_FAILURE));
                                bar.set_message(rendered);
                            }
                            bar.finish();
                        }
                        view.log.fail(message, causes);
                    }
                    // Skipped keeps the neutral style — nothing succeeded or
                    // failed.
                    TaskOutcome::Skipped { reason } => {
                        if let Some(bar) = &view.bar {
                            bar.finish_with_message(format!("Skipped ({reason})"));
                        }
                    }
                }
            }
        }
    }

    /// Let go of the live view. Its last frame stays on the terminal as
    /// scrollback, and its last line is ended, so that what is printed next
    /// starts on a line of its own.
    fn release(&mut self) {
        if self.drawn && !self.multi_progress.is_hidden() {
            self.screen.end_line();
        }
        self.drawn = false;
        self.multi_progress = self.screen.view();
    }

    /// Replay the captured output of failed tasks. Only the failing step is
    /// shown; `--debug` runs use the plain renderer, which dumps every step.
    pub(crate) fn flush(mut self) {
        self.release();
        let logs = self
            .tasks
            .into_iter()
            .map(|(id, view)| (id, view.log))
            .collect();
        dump_failures(&logs, false);
    }
}

/// Style for a byte-transfer bar.
fn transfer_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("{prefix} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
        .expect("invalid progress bar template")
        .progress_chars("#>-")
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use icp_project::operations::task::Task;
    use indicatif::{ProgressDrawTarget, TermLike};

    use super::*;

    /// A terminal that keeps what is written to it, cursor movements and
    /// all, or that is hidden, as stderr is when piped.
    #[derive(Clone, Debug, Default)]
    struct Recorded {
        written: Arc<Mutex<String>>,
        hidden: bool,
    }

    impl Recorded {
        fn written(&self) -> String {
            self.written.lock().unwrap().clone()
        }

        fn write(&self, s: &str) -> io::Result<()> {
            self.written.lock().unwrap().push_str(s);
            Ok(())
        }
    }

    impl TermLike for Recorded {
        fn width(&self) -> u16 {
            80
        }
        fn move_cursor_up(&self, n: usize) -> io::Result<()> {
            self.write(&format!("<up {n}>"))
        }
        fn move_cursor_down(&self, n: usize) -> io::Result<()> {
            self.write(&format!("<down {n}>"))
        }
        fn move_cursor_right(&self, n: usize) -> io::Result<()> {
            self.write(&format!("<right {n}>"))
        }
        fn move_cursor_left(&self, n: usize) -> io::Result<()> {
            self.write(&format!("<left {n}>"))
        }
        fn write_line(&self, s: &str) -> io::Result<()> {
            self.write(&format!("{s}\n"))
        }
        fn write_str(&self, s: &str) -> io::Result<()> {
            self.write(s)
        }
        fn clear_line(&self) -> io::Result<()> {
            self.write("<clear>")
        }
        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Screen for Recorded {
        fn view(&self) -> MultiProgress {
            MultiProgress::with_draw_target(if self.hidden {
                ProgressDrawTarget::hidden()
            } else {
                ProgressDrawTarget::term_like(Box::new(self.clone()))
            })
        }

        fn end_line(&self) {
            self.write("\n").unwrap();
        }
    }

    /// Feed a renderer the events of a build that succeeds, then a phase
    /// heading.
    fn build_then_heading(renderer: &mut InteractiveRenderer) {
        let (reporter, mut events) = icp_events::channel();
        reporter
            .task(Task::build("probe"))
            .finish(TaskOutcome::succeeded());
        while let Ok(event) = events.try_recv() {
            renderer.handle(event);
        }
        let _heading = reporter.task(Task::phase("Creating canisters:"));
        while let Ok(event) = events.try_recv() {
            renderer.handle(event);
        }
    }

    /// Indicatif leaves the cursor at the end of the last bar it drew; the
    /// renderer ends that line when it lets go of the view, at a top-level
    /// heading and at the end of the stream, so what follows starts on a
    /// line of its own. Once, and only where a bar was drawn.
    #[test]
    fn letting_go_of_a_drawn_view_ends_its_last_line() {
        let screen = Recorded::default();
        let mut renderer = InteractiveRenderer::on(Box::new(screen.clone()));
        build_then_heading(&mut renderer);
        let written = screen.written();
        assert!(written.contains("Built successfully"), "{written:?}");
        // The last frame ends on the bar itself; the line is ended after it,
        // once.
        assert!(written.ends_with('\n'), "{written:?}");
        assert_eq!(written.matches('\n').count(), 1, "{written:?}");

        // The heading's view has drawn nothing, so the end of the stream
        // ends no line.
        renderer.flush();
        assert_eq!(screen.written(), written);
    }

    /// Piped, no bar is drawn, so no line is ended: the output stays as it
    /// was.
    #[test]
    fn a_hidden_view_ends_no_line() {
        let screen = Recorded {
            hidden: true,
            ..Recorded::default()
        };
        let mut renderer = InteractiveRenderer::on(Box::new(screen.clone()));
        build_then_heading(&mut renderer);
        renderer.flush();
        assert_eq!(screen.written(), "");
    }
}
