use std::{collections::VecDeque, sync::RwLock, time::Duration};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
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

    pub fn new_progress_handler(&self, name: String) -> ScriptProgressHandler {

        let pb = self.multi_progress.add(
            ProgressBar::new_spinner()
                .with_style(
                    make_style(TICK_EMPTY, COLOR_REGULAR)
                )
        );
        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        // Set the progress bar prefix to display the canister name in brackets
        pb.set_prefix(format!("[{}]", name));

        ScriptProgressHandler {
            progress_bar: pb,
            header: RwLock::new("".to_string()),
        }
    }

}

/// Utility for handling script adapter progress with rolling terminal output
#[derive(Debug)]
pub struct ScriptProgressHandler {
    progress_bar: ProgressBar,
    header: RwLock<String>,
}

impl ScriptAdapterProgressHandler for ScriptProgressHandler {
    fn progress_update(&self, event: ScriptAdapterProgress) {
        match event {
            ScriptAdapterProgress::ScriptStarted { title } => {
                *self.header.write().unwrap() = title.clone();
                self.progress_bar.set_message(title);
            },
            ScriptAdapterProgress::Progress { line } => {
                let header = self.header.read().unwrap();
                self.progress_bar.set_message(format!("{}\n{line}\n", *header));
            },
            ScriptAdapterProgress::ScriptFinished { status, title } => {
                *self.header.write().unwrap() = title.clone();
                let header = self.header.read().unwrap();
                self.progress_bar.set_message(format!("{status} - {}", *header));
                self.progress_bar.finish();
            },
        }
    }
}

