//! Live progress-bar renderer: one indicatif spinner per task, a rolling
//! window of the current step's output beneath it, and a ✔/✘ finish state.
//! Failed tasks replay their captured output once the stream ends.

use std::{collections::BTreeMap, time::Duration};

use icp_events::{Event, EventKind, TaskId, TaskKind, TaskOutcome};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use tracing::debug;

use super::style::{
    COLOR_FAILURE, COLOR_REGULAR, COLOR_SUCCESS, TICK_EMPTY, TICK_FAILURE, TICK_SUCCESS, make_style,
};
use super::{
    RollingLines, TaskLog, dump_failures, failure_message, step_header, success_message,
    transfer_label,
};

/// Number of output lines shown live under a task's progress bar.
const LIVE_WINDOW_LINES: usize = 4;

pub(crate) struct InteractiveRenderer {
    multi_progress: MultiProgress,
    tasks: BTreeMap<TaskId, TaskView>,
}

struct TaskView {
    log: TaskLog,
    bar: ProgressBar,
    /// Header of the step currently running, shown above the live window.
    header: String,
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

    pub(crate) fn handle(&mut self, event: Event) {
        match event.kind {
            EventKind::TaskStarted { task } => {
                // Bars are configured fully before insertion: adding to the
                // MultiProgress can draw the initial frame, and it must not
                // appear unstyled or unlabeled.
                let bar = match &task {
                    // Quantifiable transfers get a byte bar instead of a
                    // spinner, labeled by the blob rather than the canister.
                    TaskKind::SnapshotTransfer {
                        blob, total_bytes, ..
                    } => self.multi_progress.add(
                        ProgressBar::new(*total_bytes)
                            .with_style(transfer_style())
                            .with_prefix(transfer_label(blob)),
                    ),
                    _ => {
                        let mut bar = ProgressBar::new_spinner()
                            .with_style(make_style(TICK_EMPTY, COLOR_REGULAR))
                            .with_prefix(format!("[{}]", task.canister()));
                        if let Some(message) = super::running_message(&task) {
                            bar = bar.with_message(message);
                        }
                        let bar = self.multi_progress.add(bar);
                        bar.enable_steady_tick(Duration::from_millis(120));
                        bar
                    }
                };

                self.tasks.insert(
                    event.task_id,
                    TaskView {
                        log: TaskLog::new(task),
                        bar,
                        header: String::new(),
                        window: RollingLines::new(LIVE_WINDOW_LINES),
                    },
                );
            }

            EventKind::Progress { position } => {
                if let Some(view) = self.tasks.get(&event.task_id) {
                    view.bar.set_position(position);
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
                view.header = step_header(view.log.kind(), number, total, &label);
                view.window = RollingLines::new(LIVE_WINDOW_LINES);
                view.log.start_step(view.header.clone());
                // The bar is deliberately not updated here: the header shows
                // once the step's first output line arrives (the Output
                // branch), so silent steps draw nothing.
            }

            EventKind::Output { line, .. } => {
                let Some(view) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };

                debug!("{line}");

                view.window.push(line.clone());
                view.log.push_line(line);

                // Update progress-bar with rolling terminal output
                // Make the output
                // │ look prettier...
                // └
                let rolled = view.window.iter().map(|s| format!("│ {s}")).join("\n");
                view.bar
                    .set_message(format!("{}\n{rolled}\n└\n\n", view.header));
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

                // A transfer's byte bar has no message or tick slot; it just
                // freezes at its final position.
                if matches!(view.log.kind(), TaskKind::SnapshotTransfer { .. }) {
                    view.bar.finish();
                    if let TaskOutcome::Failed { message, causes } = outcome {
                        view.log.fail(message, causes);
                    }
                    return;
                }

                match outcome {
                    TaskOutcome::Succeeded { retained_output } => {
                        view.bar.set_style(make_style(TICK_SUCCESS, COLOR_SUCCESS));
                        view.bar.set_message(success_message(view.log.kind()));
                        view.bar.finish();
                        super::print_retained(view.log.kind(), &retained_output);
                    }
                    TaskOutcome::Failed { message, causes } => {
                        view.bar.set_style(make_style(TICK_FAILURE, COLOR_FAILURE));
                        view.bar
                            .set_message(failure_message(view.log.kind(), &message));
                        view.bar.finish();
                        view.log.fail(message, causes);
                    }
                    // Skipped keeps the neutral style — nothing succeeded or
                    // failed.
                    TaskOutcome::Skipped { reason } => {
                        view.bar.finish_with_message(format!("Skipped ({reason})"));
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
