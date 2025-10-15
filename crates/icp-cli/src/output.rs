use console::Term;

/// Trait for outputting messages to the user
pub trait Output: Sync + Send {
    fn println(&self, msg: &str);
    fn is_debug(&self) -> bool;
}

/// Standard terminal output
pub struct TermOutput {
    term: Term,
}

impl TermOutput {
    pub fn new() -> Self {
        Self {
            term: Term::stdout(),
        }
    }
}

impl Output for TermOutput {
    fn println(&self, msg: &str) {
        let _ = self.term.write_line(msg);
    }

    fn is_debug(&self) -> bool {
        false
    }
}

/// Debug output routes everything through tracing::debug!
pub struct DebugOutput;

impl DebugOutput {
    pub fn new() -> Self {
        Self
    }
}

impl Output for DebugOutput {
    fn println(&self, msg: &str) {
        // Strip leading/trailing whitespace to avoid empty debug lines
        let msg = msg.trim();
        if !msg.is_empty() {
            tracing::debug!("{}", msg);
        }
    }

    fn is_debug(&self) -> bool {
        true
    }
}
