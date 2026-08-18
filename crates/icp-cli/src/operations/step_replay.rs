//! Replaying the output of a task's steps after it has failed.
//!
//! While a step runs, only the tail of its output is on screen, and the bar draws
//! over it as it goes. When the operation fails, that view is gone but the output is
//! the whole explanation — so the recorded steps are formatted back out, once every
//! bar has been closed, and printed as errors.

use icp_events::RecordedStep;

/// Format a failed task's captured output for printing.
///
/// `all_steps` replays the whole run rather than just the step that failed, which is
/// what `--debug` asks for. Every line is prefixed with the canister name, since
/// several canisters fail into the same output.
pub(crate) fn replay(
    canister_name: &str,
    output_label: &str,
    steps: &[RecordedStep],
    all_steps: bool,
) -> Vec<String> {
    let mut lines = vec![format!("[{canister_name}] {output_label} output:")];

    let steps = if all_steps {
        steps
    } else {
        steps.last().map(std::slice::from_ref).unwrap_or_default()
    };

    for step in steps {
        // Step titles are multi-line — the header the bar showed, plus the rolling
        // output frame around it — so only the parts with something on them are kept.
        for line in step.title.lines() {
            if !line.is_empty() {
                lines.push(format!("[{canister_name}] {line}:"));
            }
        }

        if step.lines.is_empty() {
            lines.push(format!("[{canister_name}] <no output>"));
        } else {
            lines.extend(
                step.lines
                    .iter()
                    .map(|line| format!("[{canister_name}] > {line}")),
            );
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(title: &str, lines: &[&str]) -> RecordedStep {
        RecordedStep {
            title: title.to_owned(),
            lines: lines.iter().map(|l| (*l).to_owned()).collect(),
        }
    }

    #[test]
    fn only_the_failing_step_is_replayed_by_default() {
        let steps = [step("step 1", &["ok"]), step("step 2", &["boom"])];

        assert_eq!(
            replay("backend", "Build", &steps, false),
            vec![
                "[backend] Build output:",
                "[backend] step 2:",
                "[backend] > boom",
            ]
        );
    }

    #[test]
    fn every_step_is_replayed_when_asked_for() {
        let steps = [step("step 1", &["ok"]), step("step 2", &["boom"])];

        assert_eq!(
            replay("backend", "Build", &steps, true),
            vec![
                "[backend] Build output:",
                "[backend] step 1:",
                "[backend] > ok",
                "[backend] step 2:",
                "[backend] > boom",
            ]
        );
    }

    /// A step that printed nothing says so, rather than trailing off.
    #[test]
    fn a_silent_step_is_called_out() {
        assert_eq!(
            replay("backend", "Sync", &[step("step 1", &[])], false),
            vec![
                "[backend] Sync output:",
                "[backend] step 1:",
                "[backend] <no output>"
            ]
        );
    }

    /// Sync titles start with a newline, and a step's title carries the frame the
    /// bar drew around its output; neither should show up as an empty line.
    #[test]
    fn blank_title_lines_are_dropped() {
        assert_eq!(
            replay(
                "backend",
                "Sync",
                &[step("\nSyncing: a 1 of 1\n", &["done"])],
                false
            ),
            vec![
                "[backend] Sync output:",
                "[backend] Syncing: a 1 of 1:",
                "[backend] > done",
            ]
        );
    }

    /// Nothing ran, so there is nothing to replay but the header.
    #[test]
    fn no_steps_replays_only_the_header() {
        assert_eq!(
            replay("backend", "Build", &[], false),
            vec!["[backend] Build output:"]
        );
    }
}
