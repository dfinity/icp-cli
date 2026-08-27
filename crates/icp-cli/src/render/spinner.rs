//! A direct-use spinner widget for commands that render their own UX (e.g.
//! `icp network start`) rather than consuming an event stream through a
//! [`Renderer`](super::Renderer).

use std::time::Duration;

use futures::Future;
use indicatif::{MultiProgress, ProgressBar};

use super::style::{
    COLOR_FAILURE, COLOR_REGULAR, COLOR_SUCCESS, TICK_EMPTY, TICK_FAILURE, TICK_SUCCESS, make_style,
};

/// Settings for the progress manager
pub(crate) struct ProgressManagerSettings {
    /// Whether to hide the progress bars
    pub(crate) hidden: bool,
}

/// Spinner utilities for commands that drive their own progress display.
pub(crate) struct ProgressManager {
    multi_progress: MultiProgress,
}

impl ProgressManager {
    pub(crate) fn new(settings: ProgressManagerSettings) -> Self {
        let multi_progress = MultiProgress::new();

        if settings.hidden {
            multi_progress.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        }

        Self { multi_progress }
    }

    pub(crate) fn create_independent_progress_bar(&self) -> ProgressBar {
        let pb = self
            .multi_progress
            .add(ProgressBar::new_spinner().with_style(make_style(
                TICK_EMPTY,    // end_tick
                COLOR_REGULAR, // color
            )));

        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        pb
    }

    /// Execute a task with progress tracking and automatic style updates
    pub(crate) async fn execute_with_progress<F, R, E>(
        progress_bar: &ProgressBar,
        task: F,
        success_message: impl Fn() -> String,
        error_message: impl Fn(&E) -> String,
    ) -> Result<R, E>
    where
        F: Future<Output = Result<R, E>>,
    {
        // Execute the task and capture the result
        let result = task.await;

        // Update the progress bar style and message based on result
        let (style, message) = match &result {
            Ok(_) => (make_style(TICK_SUCCESS, COLOR_SUCCESS), success_message()),
            Err(err) => (make_style(TICK_FAILURE, COLOR_FAILURE), error_message(err)),
        };

        progress_bar.set_style(style);
        progress_bar.set_message(message);
        progress_bar.finish();

        result
    }
}
