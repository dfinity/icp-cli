//! Rendering [`icp_events`] onto the terminal.
//!
//! [`IndicatifSink`] is the only place that knows both about events and about
//! `indicatif`. Operations emit events; this turns them into the same progress bars
//! the CLI has always drawn. The styles come from [`crate::progress`] so the two
//! renderers cannot drift apart while both exist.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use icp_events::{Event, EventSink, Outcome, Reporter, TaskId, TaskKind};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget};
use itertools::Itertools;
use tracing::debug;

use crate::progress::{
    RollingLines, STEADY_TICK, byte_style, failure_style, running_style, success_style,
};

/// How many lines of a step's output stay on screen while it runs.
const VISIBLE_STEP_LINES: usize = 4;

/// A [`Reporter`] that draws to the terminal.
///
/// Each call site gets its own reporter — and so its own [`MultiProgress`] — which
/// matches how `ProgressManager` was used: bars belonging to one operation are
/// grouped, and the group is torn down when the operation returns.
pub(crate) fn indicatif_reporter(hidden: bool) -> Reporter {
    Reporter::new(Arc::new(IndicatifSink::new(hidden)))
}

/// Renders [`Event`]s as `indicatif` progress bars.
pub(crate) struct IndicatifSink {
    multi: MultiProgress,
    bars: Mutex<HashMap<TaskId, BarState>>,
}

// `indicatif::ProgressBar` is not `Debug`, so neither derive nor a useful dump of
// the bars is available; report which tasks are still live instead.
impl std::fmt::Debug for IndicatifSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let live: Vec<TaskId> = match self.bars.lock() {
            Ok(bars) => bars.keys().copied().sorted().collect(),
            Err(_) => Vec::new(),
        };

        f.debug_struct("IndicatifSink")
            .field("live", &live)
            .finish()
    }
}

/// Everything needed to keep drawing one task.
struct BarState {
    bar: ProgressBar,
    /// Byte bars carry their own template, so they do not take the spinner's
    /// success/failure styles when they finish.
    styled_spinner: bool,
    /// Title of the step in progress, if any.
    step_title: Option<String>,
    /// The tail of the step's output that is currently on screen.
    visible: RollingLines,
}

impl IndicatifSink {
    /// Draw to stderr, or nowhere at all when `hidden` (that is, under `--debug`,
    /// where bars would fight with the log output).
    pub(crate) fn new(hidden: bool) -> Self {
        Self::with_draw_target(if hidden {
            ProgressDrawTarget::hidden()
        } else {
            // What `MultiProgress::new` picks anyway; spelled out so both branches
            // go through one constructor.
            ProgressDrawTarget::stderr()
        })
    }

    fn with_draw_target(target: ProgressDrawTarget) -> Self {
        let multi = MultiProgress::with_draw_target(target);

        Self {
            multi,
            bars: Mutex::new(HashMap::new()),
        }
    }

    fn start(&self, id: TaskId, kind: TaskKind, label: Option<String>) {
        let state = match kind {
            TaskKind::Bytes { total } => {
                let bar = self.multi.add(ProgressBar::new(total));
                bar.set_style(byte_style());
                // Byte bars label themselves undecorated; spinners wrap the name in
                // brackets. Both match what the code being replaced did.
                if let Some(label) = label {
                    bar.set_prefix(label);
                }

                BarState {
                    bar,
                    styled_spinner: false,
                    step_title: None,
                    visible: RollingLines::new(VISIBLE_STEP_LINES),
                }
            }
            // Spinners and multi-step bars are the same bar; only the message
            // differs, and steps build a richer one.
            _ => {
                let bar = self
                    .multi
                    .add(ProgressBar::new_spinner().with_style(running_style()));
                bar.enable_steady_tick(STEADY_TICK);
                if let Some(label) = label {
                    bar.set_prefix(format!("[{label}]"));
                }

                BarState {
                    bar,
                    styled_spinner: true,
                    step_title: None,
                    visible: RollingLines::new(VISIBLE_STEP_LINES),
                }
            }
        };

        self.bars.lock().expect("bars poisoned").insert(id, state);
    }

    fn finish(&self, id: TaskId, outcome: Outcome, message: Option<String>) {
        let Some(state) = self.bars.lock().expect("bars poisoned").remove(&id) else {
            return;
        };

        // A neutral finish leaves the style alone, and byte bars never take the
        // spinner styles at all.
        let style = match outcome {
            Outcome::Success if state.styled_spinner => Some(success_style()),
            Outcome::Failure if state.styled_spinner => Some(failure_style()),
            _ => None,
        };

        // Each arm makes the same sequence of calls the code being replaced made, so
        // that the intermediate redraws match too and not just the final frame.
        match (style, message) {
            (Some(style), message) => {
                state.bar.set_style(style);
                if let Some(message) = message {
                    state.bar.set_message(message);
                }
                state.bar.finish();
            }
            (None, Some(message)) => state.bar.finish_with_message(message),
            (None, None) => state.bar.finish(),
        }
    }

    /// Redraw the step in progress: its title, then the tail of its output.
    fn redraw_step(state: &BarState) {
        let Some(title) = &state.step_title else {
            return;
        };

        // Make the output
        // │ look prettier...
        // └
        let lines = state.visible.iter().map(|s| format!("│ {s}")).join("\n");
        state.bar.set_message(format!("{title}\n{lines}\n└\n\n"));
    }
}

impl EventSink for IndicatifSink {
    fn emit(&self, event: Event) {
        match event {
            Event::TaskStarted { id, kind, label } => self.start(id, kind, label),

            Event::TaskMessage { id, message } => {
                if let Some(state) = self.bars.lock().expect("bars poisoned").get(&id) {
                    state.bar.set_message(message);
                }
            }

            Event::TaskPosition { id, position } => {
                if let Some(state) = self.bars.lock().expect("bars poisoned").get(&id) {
                    state.bar.set_position(position);
                }
            }

            Event::StepStarted { id, title, .. } => {
                if let Some(state) = self.bars.lock().expect("bars poisoned").get_mut(&id) {
                    state.step_title = Some(title);
                    state.visible = RollingLines::new(VISIBLE_STEP_LINES);
                }
            }

            Event::StepOutput { id, line } => {
                debug!("{line}");

                if let Some(state) = self.bars.lock().expect("bars poisoned").get_mut(&id) {
                    state.visible.push(line);
                    Self::redraw_step(state);
                }
            }

            Event::StepFinished { id, .. } => {
                if let Some(state) = self.bars.lock().expect("bars poisoned").get_mut(&id) {
                    state.step_title = None;
                }
            }

            Event::TaskFinished {
                id,
                outcome,
                message,
            } => self.finish(id, outcome, message),

            // User-facing notices go through `tracing`, which the CLI's `UserLayer`
            // prints as product output. Converting the operations' `info!`/`warn!`/
            // `error!` calls to notices is a separate work item; this arm is what
            // will receive them.
            Event::Notice { level, message } => match level {
                icp_events::NoticeLevel::Warn => tracing::warn!("{message}"),
                icp_events::NoticeLevel::Error => tracing::error!("{message}"),
                _ => tracing::info!("{message}"),
            },

            _ => {}
        }
    }
}

/// Proof that the events path draws what `ProgressManager` drew.
///
/// Both renderers are pointed at the same recording terminal and driven through the
/// same logical sequence; the frames they emit are then compared byte for byte. This
/// is what backs the claim that converting the four operations left the visible CLI
/// output unchanged.
#[cfg(test)]
mod rendering_equivalence {
    use super::*;
    use crate::{
        operations::snapshot_transfer::create_transfer_progress_bar,
        progress::{ProgressManager, ProgressManagerSettings},
    };
    use futures::executor::block_on;
    use indicatif::TermLike;
    use std::io;

    /// A terminal that remembers what was written to it.
    #[derive(Debug, Clone, Default)]
    pub(super) struct RecordingTerm {
        writes: Arc<Mutex<Vec<String>>>,
    }

    /// The spinner's animation glyphs, which advance on a timer and so cannot be
    /// compared directly between two runs.
    const ANIMATION_GLYPHS: [char; 5] = ['✶', '✸', '✹', '✺', '✷'];

    impl RecordingTerm {
        /// Every frame drawn, with the spinner's animation collapsed to a single
        /// marker and the resulting consecutive duplicates removed.
        ///
        /// What survives is the sequence of *states* a bar passed through — prefix,
        /// message, and final tick — which is exactly what has to match. Two things are
        /// dropped on the way: the blank line indicatif writes to pad out the rest of
        /// the terminal row, which is a function of the frame it follows, and the
        /// animation glyph, which advances on a timer and so differs run to run.
        pub(super) fn frames(&self) -> Vec<String> {
            let mut frames: Vec<String> = self
                .writes
                .lock()
                .expect("writes poisoned")
                .iter()
                .filter(|frame| !frame.trim().is_empty())
                .map(|frame| {
                    frame
                        .chars()
                        .map(|c| {
                            if ANIMATION_GLYPHS.contains(&c) {
                                '~'
                            } else {
                                c
                            }
                        })
                        .collect()
                })
                .collect();
            frames.dedup();
            frames
        }
    }

    impl TermLike for RecordingTerm {
        fn width(&self) -> u16 {
            80
        }

        fn move_cursor_up(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_cursor_down(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_cursor_right(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn move_cursor_left(&self, _n: usize) -> io::Result<()> {
            Ok(())
        }

        fn write_line(&self, s: &str) -> io::Result<()> {
            self.writes
                .lock()
                .expect("writes poisoned")
                .push(s.to_string());
            Ok(())
        }

        fn write_str(&self, s: &str) -> io::Result<()> {
            self.writes
                .lock()
                .expect("writes poisoned")
                .push(s.to_string());
            Ok(())
        }

        fn clear_line(&self) -> io::Result<()> {
            Ok(())
        }

        fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }

    pub(super) fn recording_target(term: &RecordingTerm) -> ProgressDrawTarget {
        ProgressDrawTarget::term_like(Box::new(term.clone()))
    }

    fn old_manager(term: &RecordingTerm) -> ProgressManager {
        let manager = ProgressManager::new(ProgressManagerSettings { hidden: false });
        manager
            .multi_progress
            .set_draw_target(recording_target(term));
        manager
    }

    fn new_reporter(term: &RecordingTerm) -> Reporter {
        Reporter::new(Arc::new(IndicatifSink::with_draw_target(recording_target(
            term,
        ))))
    }

    /// Every frame `ProgressManager` drew for one canister's outcome.
    fn old_frames<E>(
        result: Result<(), E>,
        success: &str,
        error: impl Fn(&E) -> String,
    ) -> Vec<String> {
        let term = RecordingTerm::default();
        let manager = old_manager(&term);

        let bar = manager.create_progress_bar("backend");
        bar.set_message("Installing...");
        let _ = block_on(ProgressManager::execute_with_progress(
            &bar,
            async { result },
            || success.to_string(),
            error,
        ));

        term.frames()
    }

    /// Every frame the event stream draws for the same outcome.
    fn new_frames<E>(
        result: Result<(), E>,
        success: &str,
        error: impl Fn(&E) -> String,
    ) -> Vec<String> {
        let term = RecordingTerm::default();
        let reporter = new_reporter(&term);

        let task = reporter.task(TaskKind::Spinner, "backend");
        task.message("Installing...");
        let _ = block_on(task.run(async { result }, || success.to_string(), error));

        term.frames()
    }

    #[test]
    fn a_success_draws_the_same_frames_as_before() {
        let old = old_frames::<String>(Ok(()), "Installed successfully", |e| e.clone());
        let new = new_frames::<String>(Ok(()), "Installed successfully", |e| e.clone());

        assert!(
            old.iter().any(|f| f.contains("Installed successfully")),
            "old frames: {old:?}"
        );
        assert_eq!(old, new);
    }

    #[test]
    fn a_failure_draws_the_same_frames_as_before() {
        let message = "Failed to install canister: boom";
        let old = old_frames(Err("boom".to_string()), "unused", |_| message.to_string());
        let new = new_frames(Err("boom".to_string()), "unused", |_| message.to_string());

        assert!(
            old.iter().any(|f| f.contains(message)),
            "old frames: {old:?}"
        );
        assert_eq!(old, new);
    }

    /// `candid_compat` skipped a canister with `finish_with_message`, which left the
    /// running style in place and drew a single frame. A neutral finish has to do the
    /// same, down to not slipping in an extra redraw.
    #[test]
    fn a_skip_draws_the_same_frames_as_finish_with_message() {
        let term = RecordingTerm::default();
        old_manager(&term)
            .create_progress_bar("backend")
            .finish_with_message("Skipped (not an upgrade)");
        let old = term.frames();

        let term = RecordingTerm::default();
        new_reporter(&term)
            .task(TaskKind::Spinner, "backend")
            .skip("Skipped (not an upgrade)");
        let new = term.frames();

        assert!(
            old.iter().any(|f| f.contains("Skipped (not an upgrade)")),
            "old frames: {old:?}"
        );
        assert_eq!(old, new);
    }

    /// Every frame `create_transfer_progress_bar` — the pre-inversion byte bar, still used
    /// by `canister snapshot download`/`upload` — draws at `position`.
    fn old_byte_frames(position: u64) -> Vec<String> {
        let term = RecordingTerm::default();
        let bar = create_transfer_progress_bar(100, "WASM module");
        bar.set_draw_target(recording_target(&term));
        bar.set_position(position);
        bar.tick();

        term.frames()
    }

    /// Every frame a `TaskKind::Bytes` task draws at the same position.
    fn new_byte_frames(position: u64) -> Vec<String> {
        let term = RecordingTerm::default();
        let reporter = new_reporter(&term);

        let task = reporter.task(TaskKind::Bytes { total: 100 }, "WASM module");
        task.position(position);

        term.frames()
    }

    /// The byte shape is still drawn the pre-inversion way by
    /// `snapshot_transfer::create_transfer_progress_bar`. Both sides take their template
    /// from `progress::byte_style`, and this pins that they stay interchangeable.
    ///
    /// Compared at rest, where the rate reads `0 B/s` on both sides: `{wide_bar}` is given
    /// whatever width the rest of the line leaves over, so once a transfer is under way the
    /// bar's own width is a function of how long the rate string happens to be.
    #[test]
    fn a_byte_task_draws_the_same_line_as_the_transfer_bar() {
        let old = old_byte_frames(0);
        let new = new_byte_frames(0);

        assert!(
            old.last()
                .is_some_and(|frame| frame.contains("WASM module") && frame.contains("0 B/100 B")),
            "old frames: {old:?}"
        );
        assert_eq!(
            old.last().map(|frame| mask_timings(frame)),
            new.last().map(|frame| mask_timings(frame)),
            "old: {old:?}\nnew: {new:?}"
        );
    }

    /// `progress_chars` is the part of the byte template the at-rest comparison above
    /// cannot see, since nothing is filled in yet.
    #[test]
    fn both_byte_bars_fill_with_the_same_glyphs() {
        for frames in [old_byte_frames(64), new_byte_frames(64)] {
            let drawn = frames.last().expect("nothing was drawn");

            assert!(
                drawn.contains('#') && drawn.contains('>') && drawn.contains('-'),
                "{drawn:?}"
            );
        }
    }

    /// Blank out the parts of a byte bar that are derived from wall-clock time: the
    /// `[HH:MM:SS]` elapsed counter and the trailing `(rate, eta)`.
    fn mask_timings(frame: &str) -> String {
        let mut out = String::with_capacity(frame.len());
        let mut rest = frame;

        while let Some(open) = rest.find('[') {
            out.push_str(&rest[..open]);
            rest = &rest[open..];

            if is_clock(&rest[1..]) {
                out.push_str("[??:??:??]");
                rest = &rest[10..];
            } else {
                out.push('[');
                rest = &rest[1..];
            }
        }
        out.push_str(rest);

        if let Some(at) = out.rfind('(') {
            out.truncate(at);
        }
        out
    }

    /// Whether `s` opens with `HH:MM:SS]`.
    fn is_clock(s: &str) -> bool {
        let b = s.as_bytes();

        b.len() >= 9
            && b[8] == b']'
            && b[..8].iter().enumerate().all(|(i, c)| match i {
                2 | 5 => *c == b':',
                _ => c.is_ascii_digit(),
            })
    }

    /// Several canisters share one `MultiProgress`, so their bars have to be added in
    /// the same order to land on the same lines.
    #[test]
    fn bars_are_drawn_in_the_order_the_canisters_were_given() {
        let term = RecordingTerm::default();
        let manager = old_manager(&term);
        for name in ["frontend", "backend"] {
            manager
                .create_progress_bar(name)
                .finish_with_message("done");
        }
        let old = term.frames();

        let term = RecordingTerm::default();
        let reporter = new_reporter(&term);
        for name in ["frontend", "backend"] {
            reporter.task(TaskKind::Spinner, name).skip("done");
        }
        let new = term.frames();

        assert!(!old.is_empty());
        assert_eq!(old, new);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        rendering_equivalence::{RecordingTerm, recording_target},
        *,
    };

    /// Hidden bars still track state, so the sink can be exercised without a tty.
    fn sink() -> IndicatifSink {
        IndicatifSink::new(true)
    }

    fn live_bars(sink: &IndicatifSink) -> usize {
        sink.bars.lock().unwrap().len()
    }

    #[test]
    fn a_spinner_lives_from_start_to_finish() {
        let sink = sink();

        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Spinner,
            label: Some("backend".into()),
        });
        assert_eq!(live_bars(&sink), 1);

        sink.emit(Event::TaskMessage {
            id: TaskId(0),
            message: "Installing...".into(),
        });
        sink.emit(Event::TaskFinished {
            id: TaskId(0),
            outcome: Outcome::Success,
            message: Some("Installed successfully".into()),
        });

        assert_eq!(live_bars(&sink), 0);
    }

    #[test]
    fn a_label_becomes_a_bracketed_prefix() {
        let sink = sink();
        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Spinner,
            label: Some("backend".into()),
        });

        let bars = sink.bars.lock().unwrap();
        assert_eq!(bars[&TaskId(0)].bar.prefix(), "[backend]");
    }

    #[test]
    fn byte_bars_take_their_label_undecorated_and_track_position() {
        let sink = sink();
        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Bytes { total: 100 },
            label: Some("upload".into()),
        });
        sink.emit(Event::TaskPosition {
            id: TaskId(0),
            position: 64,
        });

        let bars = sink.bars.lock().unwrap();
        let state = &bars[&TaskId(0)];
        assert_eq!(state.bar.prefix(), "upload");
        assert_eq!(state.bar.position(), 64);
        assert_eq!(state.bar.length(), Some(100));
        assert!(!state.styled_spinner);
    }

    #[test]
    fn step_output_is_framed_like_the_progress_manager_did() {
        let sink = sink();
        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Steps {
                output_label: "Build".into(),
            },
            label: Some("backend".into()),
        });
        sink.emit(Event::StepStarted {
            id: TaskId(0),
            index: 0,
            title: "Building: step 1 of 1 cargo build".into(),
        });
        sink.emit(Event::StepOutput {
            id: TaskId(0),
            line: "compiling".into(),
        });

        let bars = sink.bars.lock().unwrap();
        assert_eq!(
            bars[&TaskId(0)].bar.message(),
            "Building: step 1 of 1 cargo build\n│ compiling\n└\n\n"
        );
    }

    #[test]
    fn only_the_last_few_output_lines_stay_on_screen() {
        let sink = sink();
        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Steps {
                output_label: "Build".into(),
            },
            label: None,
        });
        sink.emit(Event::StepStarted {
            id: TaskId(0),
            index: 0,
            title: "t".into(),
        });
        for i in 0..VISIBLE_STEP_LINES + 2 {
            sink.emit(Event::StepOutput {
                id: TaskId(0),
                line: format!("line {i}"),
            });
        }

        let bars = sink.bars.lock().unwrap();
        assert_eq!(
            bars[&TaskId(0)].bar.message(),
            "t\n│ line 2\n│ line 3\n│ line 4\n│ line 5\n└\n\n"
        );
    }

    #[test]
    fn a_new_step_starts_from_an_empty_screen() {
        let sink = sink();
        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Steps {
                output_label: "Build".into(),
            },
            label: None,
        });
        sink.emit(Event::StepStarted {
            id: TaskId(0),
            index: 0,
            title: "first".into(),
        });
        sink.emit(Event::StepOutput {
            id: TaskId(0),
            line: "old".into(),
        });
        sink.emit(Event::StepFinished {
            id: TaskId(0),
            index: 0,
        });
        sink.emit(Event::StepStarted {
            id: TaskId(0),
            index: 1,
            title: "second".into(),
        });
        sink.emit(Event::StepOutput {
            id: TaskId(0),
            line: "new".into(),
        });

        let bars = sink.bars.lock().unwrap();
        assert_eq!(bars[&TaskId(0)].bar.message(), "second\n│ new\n└\n\n");
    }

    /// The bar is gone from the map by the time `finish` returns, so the message has
    /// to be observed where it actually lands: on the terminal.
    #[test]
    fn a_neutral_finish_draws_its_message_before_closing_the_bar_out() {
        let term = RecordingTerm::default();
        let sink = IndicatifSink::with_draw_target(recording_target(&term));

        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Spinner,
            label: Some("backend".into()),
        });
        sink.emit(Event::TaskFinished {
            id: TaskId(0),
            outcome: Outcome::Neutral,
            message: Some("Skipped (not an upgrade)".into()),
        });

        assert_eq!(live_bars(&sink), 0);

        let frames = term.frames();
        assert!(
            frames
                .iter()
                .any(|frame| frame.contains("[backend]")
                    && frame.contains("Skipped (not an upgrade)")),
            "frames: {frames:?}"
        );
    }

    #[test]
    fn events_for_an_unknown_task_are_ignored() {
        let sink = sink();

        // Finishing twice, or reporting after a finish, must not panic.
        sink.emit(Event::TaskMessage {
            id: TaskId(99),
            message: "nobody home".into(),
        });
        sink.emit(Event::TaskFinished {
            id: TaskId(99),
            outcome: Outcome::Failure,
            message: None,
        });
    }
}
