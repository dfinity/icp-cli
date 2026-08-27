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
use icp::operations::task::{Event, Widget};

use super::{INDENT, RollingLines, TaskLog, dump_failures};

/// Number of output lines shown live under a task's progress bar.
const LIVE_WINDOW_LINES: usize = 4;

pub(crate) struct InteractiveRenderer {
    multi_progress: MultiProgress,
    tasks: BTreeMap<TaskId, TaskView>,
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
        Self {
            multi_progress: MultiProgress::new(),
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
            self.multi_progress = MultiProgress::new();
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

    /// Replay the captured output of failed tasks. Only the failing step is
    /// shown; `--debug` runs use the plain renderer, which dumps every step.
    pub(crate) fn flush(self) {
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
