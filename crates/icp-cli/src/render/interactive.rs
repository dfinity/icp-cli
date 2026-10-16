//! Live progress-bar renderer: one indicatif widget per task, a rolling
//! window of the current step's output beneath it, and a ✔/✘ finish state.
//! Nested tasks indent under their parent, and phases print as plain headings
//! above the bars. Failed tasks replay their captured output once the stream
//! ends.

use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use console::Term;
use icp_events::{EventKind, TaskId, TaskOutcome};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle, TermLike};
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
}

/// Where the live view is drawn, and lines are printed past it.
trait Screen: Send {
    /// A fresh live view.
    fn view(&self) -> MultiProgress;
    /// Whether the cursor was left at the end of a line, as indicatif leaves
    /// it after the last line of a frame it drew, rather than at the start
    /// of one.
    fn mid_line(&self) -> bool;
    /// End the line the cursor is on.
    fn end_line(&self);
    /// Print one line of output, with the view out of the way.
    fn print_line(&self, line: &str);
}

/// A terminal, with the view drawn on it when it is one. Where indicatif
/// would hide stderr (a pipe, a dumb terminal) the view is hidden, and only
/// the lines printed past it reach the terminal.
struct Terminal<T> {
    term: Tracked<T>,
    hidden: bool,
}

impl Terminal<Term> {
    /// The process's stderr.
    fn stderr() -> Self {
        Self {
            term: Tracked::new(Term::buffered_stderr()),
            hidden: ProgressDrawTarget::stderr().is_hidden(),
        }
    }
}

impl<T: TermLike + Clone + 'static> Screen for Terminal<T> {
    fn view(&self) -> MultiProgress {
        MultiProgress::with_draw_target(if self.hidden {
            ProgressDrawTarget::hidden()
        } else {
            // Redrawn at most as often as indicatif's own stderr target.
            ProgressDrawTarget::term_like_with_hz(Box::new(self.term.clone()), 20)
        })
    }

    fn mid_line(&self) -> bool {
        self.term.mid_line()
    }

    fn end_line(&self) {
        self.print_line("");
    }

    fn print_line(&self, line: &str) {
        // As `eprintln!` would, short of panicking on a closed stderr.
        let _ = self.term.write_line(line).and_then(|()| self.term.flush());
    }
}

/// A terminal that keeps track of whether its cursor was left mid-line.
/// indicatif writes the last line of a frame without ending it, so the
/// cursor stays at its end; a line ending or a cleared line puts it back at
/// the start of one. Clones share the record, so a view's draws and the
/// lines printed past it keep it current between them.
#[derive(Clone, Debug)]
struct Tracked<T> {
    term: T,
    mid_line: Arc<AtomicBool>,
}

impl<T> Tracked<T> {
    fn new(term: T) -> Self {
        Self {
            term,
            mid_line: Arc::new(AtomicBool::new(false)),
        }
    }

    fn mid_line(&self) -> bool {
        self.mid_line.load(Ordering::Acquire)
    }

    fn set_mid_line(&self, mid_line: bool) {
        self.mid_line.store(mid_line, Ordering::Release);
    }
}

impl<T: TermLike> TermLike for Tracked<T> {
    fn width(&self) -> u16 {
        self.term.width()
    }

    fn height(&self) -> u16 {
        self.term.height()
    }

    fn move_cursor_up(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_up(n)
    }

    fn move_cursor_down(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_down(n)
    }

    fn move_cursor_right(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_right(n)
    }

    fn move_cursor_left(&self, n: usize) -> io::Result<()> {
        self.term.move_cursor_left(n)
    }

    fn write_line(&self, s: &str) -> io::Result<()> {
        self.term.write_line(s)?;
        self.set_mid_line(false);
        Ok(())
    }

    fn write_str(&self, s: &str) -> io::Result<()> {
        self.term.write_str(s)?;
        if !s.is_empty() {
            self.set_mid_line(!s.ends_with(['\n', '\r']));
        }
        Ok(())
    }

    fn clear_line(&self) -> io::Result<()> {
        self.term.clear_line()?;
        self.set_mid_line(false);
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        self.term.flush()
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
        Self::on(Box::new(Terminal::stderr()))
    }

    fn on(screen: Box<dyn Screen>) -> Self {
        Self {
            multi_progress: screen.view(),
            screen,
            tasks: BTreeMap::new(),
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
                        // Above the live view, which is cleared for it and
                        // drawn again below, siblings still running included.
                        // Printed past the view's cursor instead, the lines
                        // would run on from the last bar, and the line ending
                        // that `release` adds later would follow them as a
                        // blank line.
                        if !retained_output.is_empty() {
                            let (task, screen) = (view.log.task(), &self.screen);
                            self.multi_progress.suspend(|| {
                                for line in &retained_output {
                                    screen.print_line(&super::attributed(task, line));
                                }
                            });
                        }
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
    /// scrollback, and the line it left the cursor on, if any, is ended, so
    /// that what is printed next starts on a line of its own.
    fn release(&mut self) {
        if self.screen.mid_line() {
            self.screen.end_line();
        }
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
    use std::sync::Mutex;

    use icp_project::operations::task::{Task, TransferBlob, TransferDirection};

    use super::*;

    /// A terminal that keeps what is written to it, cursor movements and
    /// all.
    #[derive(Clone, Debug, Default)]
    struct Recorded {
        written: Arc<Mutex<String>>,
    }

    impl Recorded {
        fn written(&self) -> String {
            self.written.lock().unwrap().clone()
        }

        fn write(&self, s: &str) -> io::Result<()> {
            self.written.lock().unwrap().push_str(s);
            Ok(())
        }

        /// A screen on this terminal, or a hidden one, as stderr is when
        /// piped.
        fn screen(&self, hidden: bool) -> Box<dyn Screen> {
            Box::new(Terminal {
                term: Tracked::new(self.clone()),
                hidden,
            })
        }
    }

    /// What a recording put on the screen: the text, less the cursor
    /// movements.
    fn text(written: &str) -> String {
        let mut text = String::new();
        let mut in_marker = false;
        for c in written.chars() {
            match c {
                '<' => in_marker = true,
                '>' => in_marker = false,
                c if !in_marker => text.push(c),
                _ => {}
            }
        }
        text
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

    /// Feed a renderer the events of a snapshot transfer that has made the
    /// given progress, if any, then a phase heading.
    fn transfer_then_heading(renderer: &mut InteractiveRenderer, progress: Option<u64>) {
        let (reporter, mut events) = icp_events::channel();
        let transfer = reporter.task(Task::snapshot_transfer(
            "probe",
            TransferDirection::Download,
            TransferBlob::WasmModule,
            1024,
        ));
        if let Some(position) = progress {
            transfer.progress(position);
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
        let mut renderer = InteractiveRenderer::on(screen.screen(false));
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

    /// A task's retained output (a sync plugin's stderr, say) goes above the
    /// live view, each line on its own, and the view is drawn again below
    /// it: the line ending the view at the next heading then ends the bar's
    /// line, and leaves no blank line after the output.
    #[test]
    fn retained_output_prints_above_the_view_on_lines_of_its_own() {
        let screen = Recorded::default();
        let mut renderer = InteractiveRenderer::on(screen.screen(false));
        let (reporter, mut events) = icp_events::channel();
        reporter
            .task(Task::build("probe"))
            .finish(TaskOutcome::Succeeded {
                retained_output: vec!["plugin said hi".into(), "and bye".into()],
            });
        let _heading = reporter.task(Task::phase("Creating canisters:"));
        while let Ok(event) = events.try_recv() {
            renderer.handle(event);
        }
        let written = screen.written();
        let said = written
            .find("[probe] plugin said hi\n[probe] and bye\n")
            .unwrap_or_else(|| panic!("{written:?}"));
        // The lines start where the view was cleared, with nothing of it
        // left on their row; the bar is drawn again after them, and its line
        // is ended once, with no blank line anywhere.
        let before = &written[..said];
        let since_clear = &before[before.rfind("<clear>").expect("the view is cleared")..];
        assert_eq!(text(since_clear), "", "{written:?}");
        assert!(
            written[said..].contains("✔ Built successfully"),
            "{written:?}"
        );
        assert!(written.ends_with('\n'), "{written:?}");
        assert!(!written.contains("\n\n"), "{written:?}");
    }

    /// Piped, no bar is drawn, so no line is ended: the output stays as it
    /// was.
    #[test]
    fn a_hidden_view_ends_no_line() {
        let screen = Recorded::default();
        let mut renderer = InteractiveRenderer::on(screen.screen(true));
        build_then_heading(&mut renderer);
        renderer.flush();
        assert_eq!(screen.written(), "");
    }

    /// A byte bar draws nothing until it is given progress (a spinner,
    /// nothing until its first tick), so a view let go of before then has
    /// left the cursor where it was, and ending a line would leave a blank
    /// one. What counts is what reached the terminal, not what was added to
    /// the view.
    #[test]
    fn letting_go_of_a_view_that_drew_nothing_ends_no_line() {
        let screen = Recorded::default();
        let mut renderer = InteractiveRenderer::on(screen.screen(false));
        transfer_then_heading(&mut renderer, None);
        // The view moved the cursor over its empty frame, and wrote nothing.
        assert_eq!(text(&screen.written()), "", "{:?}", screen.written());
        renderer.flush();
        assert_eq!(text(&screen.written()), "", "{:?}", screen.written());

        // Given progress, the bar is drawn, and the heading ends its line.
        let screen = Recorded::default();
        let mut renderer = InteractiveRenderer::on(screen.screen(false));
        transfer_then_heading(&mut renderer, Some(512));
        let written = screen.written();
        assert!(written.contains("512"), "{written:?}");
        assert!(written.ends_with('\n'), "{written:?}");
        assert_eq!(written.matches('\n').count(), 1, "{written:?}");
    }
}
