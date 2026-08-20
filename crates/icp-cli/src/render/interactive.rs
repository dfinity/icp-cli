//! Live progress-bar renderer: one indicatif spinner per task, a rolling
//! window of the current step's output beneath it, and a ✔/✘ finish state.
//! Failed tasks replay their captured output once the stream ends.

use std::{collections::BTreeMap, time::Duration};

use icp_events::{Event, EventKind, TaskId, TaskOutcome};
use indicatif::{MultiProgress, ProgressBar};
use itertools::Itertools;
use tracing::debug;

use crate::progress::{
    COLOR_FAILURE, COLOR_REGULAR, COLOR_SUCCESS, RollingLines, TICK_EMPTY, TICK_FAILURE,
    TICK_SUCCESS, make_style,
};

use super::{TaskLog, dump_failures, failure_message, step_header, success_message};

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
                let bar = self.multi_progress.add(
                    ProgressBar::new_spinner().with_style(make_style(TICK_EMPTY, COLOR_REGULAR)),
                );
                bar.set_prefix(format!("[{}]", task.canister()));
                if let Some(message) = super::running_message(&task) {
                    bar.set_message(message);
                }
                bar.enable_steady_tick(Duration::from_millis(120));

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
                view.bar.set_message(view.header.clone());
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
