use std::{collections::VecDeque, sync::RwLock, time::Duration};

use futures::Future;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use tokio::sync::mpsc;
use tracing::debug;
use icp_adapter::script::{ScriptAdapterProgressHandler, ScriptAdapterProgress};

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
pub struct RollingLines {
    buf: VecDeque<String>,
    capacity: usize,
}

impl RollingLines {
    /// Create a new buffer with a fixed capacity, pre-filled with empty strings.
    pub fn new(capacity: usize) -> Self {
        let mut buf = VecDeque::with_capacity(capacity);

        for _ in 0..capacity {
            buf.push_back(String::new());
        }

        Self { buf, capacity }
    }

    /// Push a new line, evicting the oldest if full.
    pub fn push(&mut self, line: String) {
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }

        self.buf.push_back(line);
    }

    /// Get an iterator over the current contents (in order).
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.buf.iter().map(|s| s.as_str())
    }
}

/// Shared progress bar utilities for build and sync commands
pub struct ProgressManager {
    pub multi_progress: MultiProgress,
}

impl ProgressManager {
    pub fn new() -> Self {
        Self {
            multi_progress: MultiProgress::new(),
        }
    }

    /// Create a new progress bar with standard configuration
    pub fn create_progress_bar(&self, canister_name: &str) -> ProgressBar {
        let pb = self
            .multi_progress
            .add(ProgressBar::new_spinner().with_style(make_style(
                TICK_EMPTY,    // end_tick
                COLOR_REGULAR, // color
            )));

        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        // Set the progress bar prefix to display the canister name in brackets
        pb.set_prefix(format!("[{}]", canister_name));

        pb
    }

    /// Execute a task with progress tracking and automatic style updates
    pub async fn execute_with_progress<F, R, E>(
        progress_bar: ProgressBar,
        task: F,
        success_message: impl Fn() -> String,
        error_message: impl Fn(&E) -> String,
    ) -> Result<R, E>
    where
        F: Future<Output = Result<R, E>>,
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
    pub async fn execute_with_custom_progress<F, R, E>(
        progress_bar: ProgressBar,
        task: F,
        success_message: impl Fn() -> String,
        error_message: impl Fn(&E) -> String,
        is_success_error: impl Fn(&E) -> bool,
    ) -> Result<R, E>
    where
        F: Future<Output = Result<R, E>>,
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

/// Utility for handling script adapter progress with rolling terminal output
#[derive(Debug)]
pub struct ScriptProgressHandler {
    pub progress_bar: ProgressBar,
    header: String,
    eheader: RwLock<String>,
}

impl ScriptAdapterProgressHandler for ScriptProgressHandler {
    fn progress_update(&self, event: ScriptAdapterProgress) {
        match event {
            ScriptAdapterProgress::ScriptStarted { title } => {
                *self.eheader.write().unwrap() = title.clone();
                self.progress_bar.set_message(title);
            },
            ScriptAdapterProgress::Progress { line } => {
                let header = self.eheader.read().unwrap();
                self.progress_bar.set_message(format!("{}\n{line}\n", *header));
            },
            ScriptAdapterProgress::ScriptFinished { status, title } => {
                *self.eheader.write().unwrap() = title.clone();
                let header = self.eheader.read().unwrap();
                self.progress_bar.set_message(format!("{status} - {}", *header));
                self.progress_bar.finish();
            },
        }
    }
}

impl ScriptProgressHandler {
    pub fn new(progress_bar: ProgressBar, header: String) -> Self {
        progress_bar.set_message(header.clone());
        Self {
            progress_bar,
            header,
            eheader: RwLock::new("".to_string()),
        }
    }

    //Temporary to keep backwards compatibility
    pub fn create(multi : &MultiProgress, canister_name: String) -> Self {
        let pb = multi.add(
            ProgressBar::new_spinner()
                .with_style(
                    make_style(TICK_EMPTY, COLOR_REGULAR)
                )
        );
        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        // Set the progress bar prefix to display the canister name in brackets
        pb.set_prefix(format!("[{}]", canister_name));

        Self {
            progress_bar: pb,
            header: "".to_string(),
            eheader: RwLock::new("".to_string()),
        }
    }

    /// Create a channel and start handling script output for progress updates
    pub fn setup_output_handler(&self) -> mpsc::Sender<String> {
        let (tx, mut rx) = mpsc::channel::<String>(100);

        // Shared progress-bar messaging utility
        let set_message = {
            let pb = self.progress_bar.clone();
            let pb_hdr = self.header.clone();

            move |msg: String| {
                pb.set_message(format!("{pb_hdr}\n{msg}\n"));
            }
        };

        // Handle logging from script commands
        tokio::spawn(async move {
            // Create a rolling buffer to contain last N lines of terminal output
            let mut lines = RollingLines::new(4);

            while let Some(line) = rx.recv().await {
                debug!(line);

                // Update output buffer
                lines.push(line);

                // Update progress-bar with rolling terminal output
                let msg = lines
                    .iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("> {}", s))
                    .join("\n");
                set_message(msg);
            }
        });

        tx
    }
}
