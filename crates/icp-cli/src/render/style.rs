//! Spinner styling shared by the interactive renderer and the direct-use
//! spinner widget.

use indicatif::ProgressStyle;

// Animation frames for the spinner - creates a rotating star effect
const TICKS: &[&str] = &["✶", "✸", "✹", "✺", "✹", "✷"];

// Final tick symbols for different completion states
pub(super) const TICK_EMPTY: &str = " ";
pub(super) const TICK_SUCCESS: &str = "✔";
pub(super) const TICK_FAILURE: &str = "✘";

// Color schemes for different progress states
pub(super) const COLOR_REGULAR: &str = "blue";
pub(super) const COLOR_SUCCESS: &str = "green";
pub(super) const COLOR_FAILURE: &str = "red";

// Creates a progress bar style with a spinner that transitions to a final tick symbol
// - end_tick: the symbol to display when the progress completes (success, failure, etc.)
// - color: the color theme for the spinner and text
pub(super) fn make_style(end_tick: &str, color: &str) -> ProgressStyle {
    // Template format: "[prefix] [spinner] [message]"
    let tmpl = format!("{{prefix}} {{spinner:.{color}}} {{msg}}");

    ProgressStyle::with_template(&tmpl)
        .expect("invalid style template")
        // Combine animation frames with the final completion symbol
        .tick_strings(&[TICKS, &[end_tick]].concat())
}
