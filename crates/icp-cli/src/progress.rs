use std::{collections::VecDeque, time::Duration};

use futures::Future;
use indicatif::{MultiProgress, ProgressBar as SimpleProgressBar, ProgressStyle};

/// The maximum number of lines to display for a step output
pub(crate) const MAX_LINES_PER_STEP: usize = 10_000;

// Animation frames for the spinner - creates a rotating star effect
const TICKS: &[&str] = &["✶", "✸", "✹", "✺", "✹", "✷"];

// Final tick symbols for different completion states
pub(crate) const TICK_EMPTY: &str = " ";
pub(crate) const TICK_SUCCESS: &str = "✔";
pub(crate) const TICK_FAILURE: &str = "✘";

// Color schemes for different progress states
pub(crate) const COLOR_REGULAR: &str = "blue";
pub(crate) const COLOR_SUCCESS: &str = "green";
pub(crate) const COLOR_FAILURE: &str = "red";

// Creates a progress bar style with a spinner that transitions to a final tick symbol
// - end_tick: the symbol to display when the progress completes (success, failure, etc.)
// - color: the color theme for the spinner and text
pub(crate) fn make_style(end_tick: &str, color: &str) -> ProgressStyle {
    // Template format: "[prefix] [spinner] [message]"
    let tmpl = format!("{{prefix}} {{spinner:.{color}}} {{msg}}");

    ProgressStyle::with_template(&tmpl)
        .expect("invalid style template")
        // Combine animation frames with the final completion symbol
        .tick_strings(&[TICKS, &[end_tick]].concat())
}

/// A fixed-capacity rolling buffer that always holds the last `capacity` items.
#[derive(Debug)]
pub(crate) struct RollingLines {
    buf: VecDeque<String>,
    capacity: usize,
}

impl RollingLines {
    /// Create a new buffer with a fixed capacity.
    pub(crate) fn new(capacity: usize) -> Self {
        let buf = VecDeque::with_capacity(capacity);
        Self { buf, capacity }
    }

    /// Push a new line, evicting the oldest if full.
    pub(crate) fn push(&mut self, line: String) {
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }

        self.buf.push_back(line);
    }

    /// Get an iterator over the current contents (in order).
    pub(crate) fn iter(&self) -> impl Iterator<Item = &str> {
        self.buf.iter().map(|s| s.as_str())
    }

    /// Whether no lines have been pushed.
    pub(crate) fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// Settings for the progress manager
pub(crate) struct ProgressManagerSettings {
    /// Whether to hide the progress bars
    pub(crate) hidden: bool,
}

/// Shared progress bar utilities for build and sync commands
pub(crate) struct ProgressManager {
    pub(crate) multi_progress: MultiProgress,
}

impl ProgressManager {
    pub(crate) fn new(settings: ProgressManagerSettings) -> Self {
        let multi_progress = MultiProgress::new();

        if settings.hidden {
            multi_progress.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }

        Self { multi_progress }
    }

    /// Create a new progress bar with standard configuration
    pub(crate) fn create_progress_bar(&self, canister_name: &str) -> SimpleProgressBar {
        let pb = self.create_independent_progress_bar();
        pb.set_prefix(format!("[{canister_name}]"));
        pb
    }

    pub(crate) fn create_independent_progress_bar(&self) -> SimpleProgressBar {
        let pb = self
            .multi_progress
            .add(SimpleProgressBar::new_spinner().with_style(make_style(
                TICK_EMPTY,    // end_tick
                COLOR_REGULAR, // color
            )));

        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        pb
    }

    /// Execute a task with progress tracking and automatic style updates
    pub(crate) async fn execute_with_progress<F, R, E, P>(
        progress_bar: &P,
        task: F,
        success_message: impl Fn() -> String,
        error_message: impl Fn(&E) -> String,
    ) -> Result<R, E>
    where
        F: Future<Output = Result<R, E>>,
        P: ProgressBar,
    {
        // Delegate to execute_with_custom_progress with no special error handling
        Self::execute_with_custom_progress(
            progress_bar,
            task,
            success_message,
            error_message,
            |_| false, // No errors are treated as success
        )
        .await
    }

    /// Execute a task with custom progress handling for errors that should display as success
    pub(crate) async fn execute_with_custom_progress<F, R, E, P>(
        progress_bar: &P,
        task: F,
        success_message: impl Fn() -> String,
        error_message: impl Fn(&E) -> String,
        is_success_error: impl Fn(&E) -> bool,
    ) -> Result<R, E>
    where
        F: Future<Output = Result<R, E>>,
        P: ProgressBar,
    {
        // Execute the task and capture the result
        let result = task.await;

        // Update the progress bar style and message based on result
        let (style, message) = match &result {
            Ok(_) => (make_style(TICK_SUCCESS, COLOR_SUCCESS), success_message()),
            Err(err) if is_success_error(err) => {
                (make_style(TICK_SUCCESS, COLOR_SUCCESS), error_message(err))
            }
            Err(err) => (make_style(TICK_FAILURE, COLOR_FAILURE), error_message(err)),
        };

        progress_bar.set_style(style);
        progress_bar.set_message(message);
        progress_bar.finish();

        result
    }
}

pub(crate) trait ProgressBar {
    fn set_style(&self, style: ProgressStyle);
    fn set_message(&self, message: String);
    fn finish(&self);
}

impl ProgressBar for SimpleProgressBar {
    fn set_style(&self, style: ProgressStyle) {
        SimpleProgressBar::set_style(self, style);
    }

    fn set_message(&self, message: String) {
        SimpleProgressBar::set_message(self, message);
    }

    fn finish(&self) {
        SimpleProgressBar::finish(self);
    }
}
