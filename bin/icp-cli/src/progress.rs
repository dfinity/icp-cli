use std::time::Duration;

use futures::Future;
use indicatif::{MultiProgress, ProgressBar};
use itertools::Itertools;
use tokio::sync::mpsc;
use tracing::debug;

use crate::{
    COLOR_FAILURE, COLOR_REGULAR, COLOR_SUCCESS, RollingLines, TICK_EMPTY, TICK_FAILURE,
    TICK_SUCCESS, make_style,
};

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
        // Execute the task and capture the result
        let result = task.await;

        // Update the progress bar style based on result
        progress_bar.set_style(match &result {
            Ok(_) => make_style(TICK_SUCCESS, COLOR_SUCCESS),
            Err(_) => make_style(TICK_FAILURE, COLOR_FAILURE),
        });

        // Update the progress bar message based on result
        progress_bar.set_message(match &result {
            Ok(_) => success_message(),
            Err(err) => error_message(err),
        });

        // Stop the progress bar spinner and keep the final state visible
        progress_bar.finish();

        result
    }
}

/// Utility for handling script adapter progress with rolling terminal output
pub struct ScriptProgressHandler {
    progress_bar: ProgressBar,
    header: String,
}

impl ScriptProgressHandler {
    pub fn new(progress_bar: ProgressBar, header: String) -> Self {
        progress_bar.set_message(header.clone());
        Self {
            progress_bar,
            header,
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
                pb.set_message(format!("{pb_hdr}\n\n{msg}\n"));
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
                let msg = lines.iter().join("\n");
                set_message(msg);
            }
        });

        tx
    }
}
