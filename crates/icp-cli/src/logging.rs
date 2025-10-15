use console::Term;
use tracing::debug;

pub struct Terminal {
    term: Term,
    debug: bool,
}

impl Terminal {
    pub fn new(debug: bool) -> Self {
        Self {
            term: Term::stdout(),
            debug,
        }
    }
}

impl Terminal {
    /// Print a message to the user on stdout and add a new line
    pub fn write_line(&self, msg: &str) {
        // Only print using term if debug is disabled
        if !self.debug {
            let _ = self.term.write_line(msg);
        }
        debug!("{msg}");
    }

    pub fn is_debug(&self) -> bool {
        self.debug
    }
}
