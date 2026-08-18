//! Rendering [`icp_events`] onto the terminal.
//!
//! This is the only module that knows about `indicatif`. Operations emit events;
//! [`IndicatifSink`] turns them into the progress bars the CLI has always drawn, and
//! the styles they are drawn in live here with it.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use icp_events::{Event, EventSink, Outcome, Reporter, TaskId, TaskKind};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use itertools::Itertools;
use tracing::debug;

/// How many lines of a step's output stay on screen while it runs.
const VISIBLE_STEP_LINES: usize = 4;

/// Animation frames for the spinner - creates a rotating star effect
const TICKS: &[&str] = &["✶", "✸", "✹", "✺", "✹", "✷"];

// Final tick symbols for different completion states
const TICK_EMPTY: &str = " ";
const TICK_SUCCESS: &str = "✔";
const TICK_FAILURE: &str = "✘";

// Color schemes for different progress states
const COLOR_REGULAR: &str = "blue";
const COLOR_SUCCESS: &str = "green";
const COLOR_FAILURE: &str = "red";

/// How often a running spinner redraws itself.
const STEADY_TICK: Duration = Duration::from_millis(120);

/// The style a spinner carries while it is still running.
fn running_style() -> ProgressStyle {
    make_style(TICK_EMPTY, COLOR_REGULAR)
}

/// The style a spinner carries once it has succeeded.
fn success_style() -> ProgressStyle {
    make_style(TICK_SUCCESS, COLOR_SUCCESS)
}

/// The style a spinner carries once it has failed.
fn failure_style() -> ProgressStyle {
    make_style(TICK_FAILURE, COLOR_FAILURE)
}

/// The style a byte-transfer bar carries.
fn byte_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("{prefix} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
        .expect("invalid progress bar template")
        .progress_chars("#>-")
}

// Creates a progress bar style with a spinner that transitions to a final tick symbol
// - end_tick: the symbol to display when the progress completes (success, failure, etc.)
// - color: the color theme for the spinner and text
fn make_style(end_tick: &str, color: &str) -> ProgressStyle {
    // Template format: "[prefix] [spinner] [message]"
    let tmpl = format!("{{prefix}} {{spinner:.{color}}} {{msg}}");

    ProgressStyle::with_template(&tmpl)
        .expect("invalid style template")
        // Combine animation frames with the final completion symbol
        .tick_strings(&[TICKS, &[end_tick]].concat())
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
}

/// A [`Reporter`] that draws to the terminal.
///
/// Each call site gets its own reporter — and so its own [`MultiProgress`] — which
/// matches how the progress manager it replaced was used: bars belonging to one
/// operation are grouped, and the group is torn down when the operation returns.
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
        // Every bar is fully styled and labelled *before* it can draw anything: only
        // then is it added to the `MultiProgress` and, for spinners, given a ticker.
        // `Ticker::new` ticks immediately on the thread it spawns, so a prefix set
        // after `enable_steady_tick` races that first tick and can lose — which is
        // what drew a stray unprefixed spinner frame. See
        // `tests::a_spinner_is_labelled_before_its_first_tick`.
        let state = match kind {
            TaskKind::Bytes { total } => {
                // Byte bars label themselves undecorated; spinners wrap the name in
                // brackets. Both match what the code being replaced did.
                let bar = ProgressBar::new(total).with_style(byte_style());
                let bar = match label {
                    Some(label) => bar.with_prefix(label),
                    None => bar,
                };

                BarState {
                    bar: self.multi.add(bar),
                    styled_spinner: false,
                    step_title: None,
                    visible: RollingLines::new(VISIBLE_STEP_LINES),
                }
            }
            // Spinners and multi-step bars are the same bar; only the message
            // differs, and steps build a richer one.
            _ => {
                let bar = ProgressBar::new_spinner().with_style(running_style());
                let bar = match label {
                    Some(label) => bar.with_prefix(format!("[{label}]")),
                    None => bar,
                };

                let bar = self.multi.add(bar);
                bar.enable_steady_tick(STEADY_TICK);

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

    /// Run `f` against the bar belonging to `id`.
    ///
    /// A task with no bar is not an error: an event can arrive after its task has
    /// finished and been removed, and the code being replaced ignored those too.
    fn with_bar(&self, id: TaskId, f: impl FnOnce(&mut BarState)) {
        if let Some(state) = self.bars.lock().expect("bars poisoned").get_mut(&id) {
            f(state);
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
                self.with_bar(id, |state| state.bar.set_message(message));
            }

            Event::TaskPosition { id, position } => {
                self.with_bar(id, |state| state.bar.set_position(position));
            }

            Event::StepStarted { id, title, .. } => self.with_bar(id, |state| {
                state.step_title = Some(title);
                state.visible = RollingLines::new(VISIBLE_STEP_LINES);
            }),

            Event::StepOutput { id, line } => {
                debug!("{line}");

                self.with_bar(id, |state| {
                    state.visible.push(line);
                    Self::redraw_step(state);
                });
            }

            Event::StepFinished { id, .. } => {
                self.with_bar(id, |state| state.step_title = None);
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

/// Proof that the events path draws what the code it replaced drew.
///
/// The pre-inversion renderers — `progress::ProgressManager` and
/// `snapshot_transfer::create_transfer_progress_bar` — are deleted in the same change
/// that adds this, so there is nothing left to compare against at runtime. Instead,
/// every `EXPECTED_*` below is the literal output those renderers produced, captured
/// from them before they were removed by pointing them at the same [`RecordingTerm`]
/// and driving them through the same logical sequence. A frame that changes here is a
/// frame that changed for the user.
#[cfg(test)]
mod rendering {
    use super::*;
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

    /// Drop the colour codes from a frame.
    ///
    /// `indicatif` only calls `ProgressStyle::set_for_stderr` when its draw target
    /// *is* stderr, which a [`TermLike`] target never is, so the styled template
    /// fields fall back to `console`'s stdout-based colour detection: run from a
    /// terminal the frames arrive wrapped in SGR codes, run from a pipe they do not.
    /// What is drawn does not depend on how the developer started `cargo test`, so
    /// the codes come off before anything else looks at the frame.
    fn strip_ansi(frame: &str) -> String {
        let mut out = String::with_capacity(frame.len());
        let mut chars = frame.chars();

        while let Some(c) = chars.next() {
            if c != '\u{1b}' {
                out.push(c);
                continue;
            }

            // A CSI sequence — `ESC [`, which is all `console` emits — runs up to and
            // including its final byte in `@..=~`. Any other escape is two characters.
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        }

        out
    }

    impl RecordingTerm {
        /// Every frame drawn, with the spinner's animation collapsed to a single
        /// marker and the resulting consecutive duplicates removed.
        ///
        /// What survives is the sequence of *states* a bar passed through — prefix,
        /// message, and final tick — which is exactly what has to match. Four things
        /// are dropped on the way:
        ///
        /// - the colour codes, which depend on whether the test binary's stdout is a
        ///   terminal rather than on what was drawn (see [`strip_ansi`]);
        /// - the blank line indicatif writes to pad out the rest of the terminal row,
        ///   which is a function of the frame it follows;
        /// - the animation glyph, which advances on a timer and so differs run to run;
        /// - a spinner frame with no message yet, which is the animation thread
        ///   getting a frame in before the operation has said anything. Whether that
        ///   happens is scheduling, not behaviour: the frame is overwritten by the
        ///   next one either way.
        pub(super) fn frames(&self) -> Vec<String> {
            let mut frames: Vec<String> = self
                .writes
                .lock()
                .expect("writes poisoned")
                .iter()
                .map(|frame| strip_ansi(frame))
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
                        .collect::<String>()
                })
                .filter(|frame| !frame.trim_end().ends_with('~'))
                .collect();
            frames.dedup();
            frames
        }

        /// Every frame drawn, untouched apart from dropping the blank padding lines
        /// and the environment-dependent colour codes.
        pub(super) fn raw_frames(&self) -> Vec<String> {
            self.writes
                .lock()
                .expect("writes poisoned")
                .iter()
                .map(|frame| strip_ansi(frame))
                .filter(|frame| !frame.trim().is_empty())
                .collect()
        }

        /// [`frames`](Self::frames), with everything clock-derived blanked out, for
        /// the byte bar.
        pub(super) fn timeless_frames(&self) -> Vec<String> {
            let mut frames: Vec<String> = self.frames().iter().map(mask_timings).collect();
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

    fn new_reporter(term: &RecordingTerm) -> Reporter {
        Reporter::new(Arc::new(IndicatifSink::with_draw_target(recording_target(
            term,
        ))))
    }

    /// Whether the recorded frames carry colour codes is a property of the machine
    /// the tests run on, not of the sink: a [`TermLike`] draw target is never stderr,
    /// so `indicatif` leaves the styled fields following `console`'s stdout-based
    /// detection, and the goldens below would only hold when `cargo test` was piped.
    /// Both views have to normalize that away before the animation-glyph and
    /// message-less-frame rules can see anything either.
    #[test]
    fn normalization_strips_the_colour_codes_a_terminal_would_add() {
        let term = RecordingTerm::default();
        for line in [
            "[backend] \u{1b}[34m✶\u{1b}[0m ",
            "[backend] \u{1b}[34m✶\u{1b}[0m Installing...",
            "[backend] \u{1b}[32m✔\u{1b}[0m Installed successfully",
        ] {
            term.write_line(line).expect("recording cannot fail");
        }

        assert_eq!(
            term.raw_frames(),
            [
                "[backend] ✶ ",
                "[backend] ✶ Installing...",
                "[backend] ✔ Installed successfully",
            ]
        );

        // The message-less frame still drops out, which the trailing-glyph rule can
        // only tell once the reset code sitting after that glyph is gone.
        assert_eq!(
            term.frames(),
            [
                "[backend] ~ Installing...",
                "[backend] ✔ Installed successfully",
            ]
        );
    }

    /// Every frame the event stream draws for one canister's outcome.
    fn frames_for<E>(
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

    /// What `ProgressManager` drew for a canister that installed successfully.
    const EXPECTED_SUCCESS: [&str; 3] = [
        "[backend] ~ Installing...",
        "[backend] ~ Installed successfully",
        "[backend] ✔ Installed successfully",
    ];

    #[test]
    fn a_success_draws_the_frames_the_progress_manager_drew() {
        assert_eq!(
            frames_for::<String>(Ok(()), "Installed successfully", |e| e.clone()),
            EXPECTED_SUCCESS
        );
    }

    /// What `ProgressManager` drew for a canister that failed to install.
    const EXPECTED_FAILURE: [&str; 3] = [
        "[backend] ~ Installing...",
        "[backend] ~ Failed to install canister: boom",
        "[backend] ✘ Failed to install canister: boom",
    ];

    #[test]
    fn a_failure_draws_the_frames_the_progress_manager_drew() {
        let message = "Failed to install canister: boom";

        assert_eq!(
            frames_for(Err("boom".to_string()), "unused", |_| message.to_string()),
            EXPECTED_FAILURE
        );
    }

    /// `candid_compat` skipped a canister with `finish_with_message`, which left the
    /// running style in place and drew a single frame. A neutral finish has to do the
    /// same, down to not slipping in an extra redraw.
    #[test]
    fn a_skip_draws_what_finish_with_message_drew() {
        let term = RecordingTerm::default();
        new_reporter(&term)
            .task(TaskKind::Spinner, "backend")
            .skip("Skipped (not an upgrade)");

        assert_eq!(term.frames(), ["[backend]   Skipped (not an upgrade)"]);
    }

    /// Several canisters share one `MultiProgress`, so their bars have to be added in
    /// the same order to land on the same lines.
    #[test]
    fn bars_are_drawn_in_the_order_the_canisters_were_given() {
        let term = RecordingTerm::default();
        let reporter = new_reporter(&term);
        for name in ["frontend", "backend"] {
            reporter.task(TaskKind::Spinner, name).skip("done");
        }

        assert_eq!(term.frames(), ["[frontend]   done", "[backend]   done"]);
    }

    /// Every frame a [`TaskKind::Bytes`] task draws, with the clock blanked out.
    fn byte_frames(positions: &[u64], finish: bool) -> Vec<String> {
        let term = RecordingTerm::default();
        let reporter = new_reporter(&term);

        let task = reporter.task(TaskKind::Bytes { total: 100 }, "WASM module");
        for position in positions {
            task.position(*position);
        }
        if finish {
            task.succeed("done");
            return term.timeless_frames();
        }

        // Read the frames while the task is still alive: finishing a byte bar fills it
        // to its length, and an unfinished transfer has not got there yet.
        term.timeless_frames()
    }

    /// The line `create_transfer_progress_bar` drew for a transfer at rest.
    ///
    /// `{wide_bar}` is given whatever width the rest of the line leaves over, so the
    /// bar's own width is a function of how long the rate string happens to be — hence
    /// comparing at rest, where the rate reads `0 B/s` either way.
    #[test]
    fn a_byte_task_draws_the_transfer_bars_line() {
        assert_eq!(
            byte_frames(&[0], false).first().map(String::as_str),
            Some("WASM module [??:??:??] [---------------------------------] 0 B/100 B ")
        );
    }

    /// `progress_chars` is the part of the byte template the at-rest comparison above
    /// cannot see, since nothing is filled in yet.
    ///
    /// Only the glyphs are pinned, not how many of each: `{wide_bar}` takes whatever
    /// width the rest of the line leaves over, and once bytes have moved that includes
    /// a transfer rate whose text is as long as the machine happened to be fast.
    #[test]
    fn a_byte_task_fills_with_the_transfer_bars_glyphs() {
        let drawn = byte_frames(&[64], false).pop().expect("nothing was drawn");

        assert!(
            drawn.contains("WASM module")
                && drawn.contains("64 B/100 B")
                && drawn.contains('#')
                && drawn.contains('>')
                && drawn.contains('-'),
            "{drawn:?}"
        );
    }

    /// A resumed transfer starts partway in: `snapshot_transfer` reports the frontier
    /// it recovered from disk before reporting any new bytes, so the bar opens at that
    /// offset instead of counting up from zero. Its last frame is full, because
    /// finishing a byte bar completes it.
    #[test]
    fn a_resumed_byte_task_starts_from_its_offset() {
        assert_eq!(
            byte_counters(&byte_frames(&[40, 72], true)),
            ["40 B/100 B", "72 B/100 B", "100 B/100 B"]
        );
    }

    /// The `<done>/<total>` counter of each frame, which — unlike the width of the bar
    /// beside it — does not depend on how fast the transfer went.
    fn byte_counters(frames: &[String]) -> Vec<String> {
        frames
            .iter()
            .map(|frame| {
                let after_bar = frame
                    .rsplit_once(']')
                    .expect("a byte frame always draws its bar")
                    .1;
                after_bar.trim().to_owned()
            })
            .collect()
    }

    /// Blank out the parts of a byte bar that are derived from wall-clock time: the
    /// `[HH:MM:SS]` elapsed counter and the trailing `(rate, eta)`.
    fn mask_timings(frame: impl AsRef<str>) -> String {
        let frame = frame.as_ref();
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
}

#[cfg(test)]
mod tests {
    use super::{
        rendering::{RecordingTerm, recording_target},
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

    /// `enable_steady_tick` spawns a thread that draws a frame straight away, so a
    /// bar labelled after that call races its own first tick — and a slow enough
    /// machine loses, drawing a bare spinner before the label appears. Waiting out
    /// several tick intervals here means the ticker has certainly drawn: every frame
    /// it produced has to carry the prefix.
    #[test]
    fn a_spinner_is_labelled_before_its_first_tick() {
        let term = RecordingTerm::default();
        let sink = IndicatifSink::with_draw_target(recording_target(&term));

        sink.emit(Event::TaskStarted {
            id: TaskId(0),
            kind: TaskKind::Spinner,
            label: Some("backend".into()),
        });
        std::thread::sleep(STEADY_TICK * 3);

        // Raw frames, not the normalized ones: the point is that no frame was ever
        // drawn without the label, including the message-less ones.
        let frames = term.raw_frames();
        assert!(!frames.is_empty(), "the ticker never drew anything");
        assert!(
            frames.iter().all(|frame| frame.contains("[backend]")),
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
