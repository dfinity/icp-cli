//! Renderer for `--debug` runs: no live progress bars (they would interleave
//! with the debug log). Output lines go to the debug log as they arrive, and
//! failed tasks dump the captured output of every step once the stream ends.

use std::collections::BTreeMap;

use icp_events::{EventKind, TaskId, TaskOutcome};
use tracing::{debug, info};

use icp::operations::task::{Event, Widget};

use super::{INDENT, TaskLog, dump_failures};

pub(crate) struct PlainRenderer {
    tasks: BTreeMap<TaskId, TaskLog>,
}

impl PlainRenderer {
    pub(crate) fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
        }
    }

    /// How deeply a task nests, from its parent's depth. A task whose parent
    /// is unknown is treated as top-level.
    fn depth_of(&self, parent: Option<TaskId>) -> usize {
        parent
            .and_then(|id| self.tasks.get(&id))
            .map(|log| log.depth() + 1)
            .unwrap_or(0)
    }

    pub(crate) fn handle(&mut self, event: Event) {
        match event.kind {
            EventKind::TaskStarted { parent, task } => {
                let depth = self.depth_of(parent);
                // A heading has no live state to animate, so with the bars
                // gone it is simply a line.
                if let Widget::Heading { title } = task.presentation().widget() {
                    info!("{}{title}", INDENT.repeat(depth));
                }
                self.tasks.insert(event.task_id, TaskLog::new(task, depth));
            }

            EventKind::StepStarted {
                number,
                total,
                label,
            } => {
                let Some(log) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };
                let header = log.presentation().step_header(number, total, &label);
                log.start_step(header);
            }

            EventKind::CommandStarted { command } => {
                let Some(log) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };
                // Mark command boundaries so interleaved output stays
                // attributable to the command producing it.
                debug!("{}", log.line(&format!("$ {command}")));
            }

            EventKind::Output { line, .. } => {
                let Some(log) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };
                // Prefix with the canister so interleaved concurrent tasks
                // stay attributable.
                debug!("{}", log.line(&line));
                log.push_line(line);
            }

            EventKind::StepCompleted { .. } => {
                if let Some(log) = self.tasks.get_mut(&event.task_id) {
                    log.end_step();
                }
            }

            // No live display to advance.
            EventKind::Progress { .. } => {}

            EventKind::TaskCompleted { outcome } => {
                let Some(log) = self.tasks.get_mut(&event.task_id) else {
                    return;
                };
                match outcome {
                    TaskOutcome::Succeeded { retained_output } => {
                        super::print_retained(log.task(), &retained_output);
                    }
                    TaskOutcome::Failed { message, causes } => {
                        log.fail(message, causes);
                    }
                    TaskOutcome::Skipped { .. } => {}
                }
            }
        }
    }

    /// Replay the captured output of failed tasks, including every step.
    pub(crate) fn flush(self) {
        dump_failures(&self.tasks, true);
    }
}
