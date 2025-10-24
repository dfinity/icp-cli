use std::{collections::VecDeque, time::Duration};

use futures::Future;
use indicatif::{MultiProgress, ProgressBar as SimpleProgressBar, ProgressStyle};
use itertools::Itertools;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::debug;

/// The maximum number of lines to display for a step output
pub(crate) const MAX_LINES_PER_STEP: usize = 10_000;

// Animation frames for the spinner - creates a rotating star effect
const TICKS: &[&str] = &["✶", "✸", "✹", "✺", "✹", "✷"];

// Final tick symbols for different completion states
const TICK_EMPTY: &str = " ";
const TICK_SUCCESS: &str = "✔";
const TICK_FAILURE: &str = "✘";

// Color schemes for different progress states
const COLOR_REGULAR: &str = "blue";
const COLOR_SUCCESS: &str = "green";
const COLOR_FAILURE: &str = "red";

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

    /// Convert the buffer into an iterator (in order).
    pub(crate) fn into_iter(self) -> impl Iterator<Item = String> {
        self.buf.into_iter()
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
        let pb = self
            .multi_progress
            .add(SimpleProgressBar::new_spinner().with_style(make_style(
                TICK_EMPTY,    // end_tick
                COLOR_REGULAR, // color
            )));

        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        // Set the progress bar prefix to display the canister name in brackets
        pb.set_prefix(format!("[{canister_name}]"));

        pb
    }

    /// Create a new progress bar for a multi-step operation.
    pub(crate) fn create_multi_step_progress_bar(
        &self,
        canister_name: &str,
        output_label: &str,
    ) -> MultiStepProgressBar {
        MultiStepProgressBar {
            progress_bar: self.create_progress_bar(canister_name),
            canister_name: canister_name.to_string(),
            output_label: output_label.to_string(),
            finished_steps: Vec::new(),
            in_progress: None,
        }
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

struct StepOutput {
    title: String,
    output: Vec<String>,
}

struct StepInProgress {
    title: String,
    receiver: JoinHandle<Vec<String>>,
}

pub(crate) struct MultiStepProgressBar {
    progress_bar: SimpleProgressBar,
    canister_name: String,
    output_label: String,
    finished_steps: Vec<StepOutput>,
    in_progress: Option<StepInProgress>,
}

impl MultiStepProgressBar {
    pub(crate) fn begin_step(&mut self, title: String) -> mpsc::Sender<String> {
        if self.in_progress.is_some() {
            panic!("step already in progress");
        }

        let (tx, mut rx) = mpsc::channel::<String>(100);

        let set_message = {
            let pb = self.progress_bar.clone();
            let title = title.clone();

            move |msg: String| {
                pb.set_message(format!("{title}\n{msg}\n"));
            }
        };

        // Handle logging from script commands
        let handle = tokio::spawn(async move {
            // Small rolling buffer to display current output while build is ongoing
            let mut rolling = RollingLines::new(4);
            // Total output buffer to display full build output later
            let mut complete = RollingLines::new(MAX_LINES_PER_STEP); // We need _some_ limit to prevent consuming infinite memory

            while let Some(line) = rx.recv().await {
                debug!("{line}");

                // Update output buffer
                rolling.push(line.clone());
                complete.push(line);

                // Update progress-bar with rolling terminal output
                let msg = rolling.iter().map(|s| format!("> {s}")).join("\n");
                set_message(msg);
            }

            complete.into_iter().collect()
        });

        self.in_progress = Some(StepInProgress {
            title,
            receiver: handle,
        });

        tx
    }

    pub(crate) async fn end_step(&mut self) {
        let StepInProgress { title, receiver } =
            self.in_progress.take().expect("no step in progress");
        let output = receiver.await.unwrap();

        self.finished_steps.push(StepOutput { title, output });
    }

    pub(crate) fn dump_output(&self) -> Vec<String> {
        let mut lines = Vec::new();

        lines.push(format!(
            "[{}] {} output:",
            self.canister_name, self.output_label
        ));

        for step_output in self.finished_steps.iter() {
            for line in step_output.title.lines() {
                if !line.is_empty() {
                    lines.push(format!("[{}] {}:", self.canister_name, line));
                }
            }

            if step_output.output.is_empty() {
                lines.push(format!("[{}] <no output>", self.canister_name));
            } else {
                lines.extend(
                    step_output
                        .output
                        .iter()
                        .map(|s| format!("[{}] > {s}", self.canister_name)),
                );
            }
        }

        lines
    }
}

pub(crate) trait ProgressBar {
    fn set_style(&self, style: ProgressStyle);
    fn set_message(&self, message: String);
    fn finish(&self);
}

impl ProgressBar for MultiStepProgressBar {
    fn set_style(&self, style: ProgressStyle) {
        self.progress_bar.set_style(style);
    }

    fn set_message(&self, message: String) {
        self.progress_bar.set_message(message);
    }

    fn finish(&self) {
        self.progress_bar.finish();
    }
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
