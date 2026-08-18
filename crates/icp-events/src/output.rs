use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::{
    event::{Event, TaskId},
    reporter::Reporter,
};

/// How many of a step's output lines are kept for replay.
///
/// Some limit is needed: a runaway command can print without end. Once a step is
/// over this, its oldest lines are dropped and the most recent ones survive, which
/// is the half worth showing after a failure.
pub const MAX_RECORDED_LINES_PER_STEP: usize = 10_000;

/// One step's title and the output it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedStep {
    /// The title the step was started with.
    pub title: String,

    /// Every line the step reported, oldest first, capped at
    /// [`MAX_RECORDED_LINES_PER_STEP`].
    pub lines: Vec<String>,
}

/// A step being recorded. Holds its lines in a ring so the cap costs nothing.
#[derive(Debug)]
struct Recording {
    title: String,
    lines: VecDeque<String>,
}

/// The output of every step of one task, in the order the steps ran.
///
/// Progress is transient — a bar redraws over it — but an operation that fails
/// wants the whole of the failing step back so it can print it once the bars are
/// gone. Keeping that here means each operation does not have to tee the lines it
/// hands out.
#[derive(Debug, Default)]
pub(crate) struct StepLog {
    steps: Mutex<Vec<Recording>>,
}

impl StepLog {
    /// Open a new step to record against.
    pub(crate) fn begin(&self, title: String) {
        self.steps
            .lock()
            .expect("step log poisoned")
            .push(Recording {
                title,
                lines: VecDeque::new(),
            });
    }

    /// Record a line against the step most recently opened.
    ///
    /// Lines that arrive with no step open are dropped: they have nowhere to be
    /// replayed from. This is possible but unusual — a command whose output
    /// outlives the step that ran it.
    pub(crate) fn record(&self, line: &str) {
        let mut steps = self.steps.lock().expect("step log poisoned");

        if let Some(step) = steps.last_mut() {
            if step.lines.len() == MAX_RECORDED_LINES_PER_STEP {
                step.lines.pop_front();
            }
            step.lines.push_back(line.to_owned());
        }
    }

    /// Every step recorded so far.
    pub(crate) fn recorded(&self) -> Vec<RecordedStep> {
        self.steps
            .lock()
            .expect("step log poisoned")
            .iter()
            .map(|step| RecordedStep {
                title: step.title.clone(),
                lines: step.lines.iter().cloned().collect(),
            })
            .collect()
    }
}

/// Where a running command's output lines go.
///
/// This is what crosses a crate boundary: a library that runs a subprocess or a
/// plugin takes an `OutputWriter` and reports each line it reads, learning nothing
/// about what renders them. Cloning is cheap, and every clone writes to the same
/// task, so a writer can be handed to as many concurrent readers as a command has
/// output streams.
#[derive(Debug, Clone)]
pub struct OutputWriter {
    reporter: Reporter,
    id: TaskId,
    log: Arc<StepLog>,
}

impl OutputWriter {
    pub(crate) fn new(reporter: Reporter, id: TaskId, log: Arc<StepLog>) -> Self {
        Self { reporter, id, log }
    }

    /// Report one line of output.
    ///
    /// Never blocks and never fails: with nothing rendering, a line simply goes
    /// nowhere.
    pub fn line(&self, line: impl Into<String>) {
        let line = line.into();
        self.log.record(&line);
        self.reporter.emit(Event::StepOutput { id: self.id, line });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> StepLog {
        StepLog::default()
    }

    #[test]
    fn steps_are_recorded_in_order_with_their_lines() {
        let log = log();

        log.begin("first".to_owned());
        log.record("a");
        log.record("b");
        log.begin("second".to_owned());
        log.record("c");

        assert_eq!(
            log.recorded(),
            vec![
                RecordedStep {
                    title: "first".to_owned(),
                    lines: vec!["a".to_owned(), "b".to_owned()],
                },
                RecordedStep {
                    title: "second".to_owned(),
                    lines: vec!["c".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn a_step_keeps_its_most_recent_lines_once_capped() {
        let log = log();
        log.begin("noisy".to_owned());

        for i in 0..MAX_RECORDED_LINES_PER_STEP + 2 {
            log.record(&format!("line {i}"));
        }

        let recorded = log.recorded();
        let lines = &recorded[0].lines;
        assert_eq!(lines.len(), MAX_RECORDED_LINES_PER_STEP);
        assert_eq!(lines.first().unwrap(), "line 2");
        assert_eq!(
            lines.last().unwrap(),
            &format!("line {}", MAX_RECORDED_LINES_PER_STEP + 1)
        );
    }

    #[test]
    fn lines_with_no_step_open_are_dropped() {
        let log = log();
        log.record("nowhere to go");

        assert!(log.recorded().is_empty());
    }
}
