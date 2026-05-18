use std::fmt;
use std::io::{self, IsTerminal, Write, stderr};

use anstyle::{AnsiColor, Style};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::filter::{Filtered, Targets};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, fmt::format};

fn should_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && stderr().is_terminal()
}

// Debug layer (used with --debug)

type DebugLayer<S> = Filtered<
    tracing_subscriber::fmt::Layer<
        S,
        format::DefaultFields,
        format::Format<format::Full, ()>,
        fn() -> io::Stderr,
    >,
    Targets,
    S,
>;

pub(crate) fn debug_layer<S: Subscriber + for<'a> LookupSpan<'a>>() -> DebugLayer<S> {
    let workspace_targets = Targets::new()
        .with_default(Level::WARN)
        .with_target("icp", Level::DEBUG);

    tracing_subscriber::fmt::layer()
        .with_writer(io::stderr as _)
        .without_time()
        .with_filter(workspace_targets)
}

// User-facing layer (always active, info/warn/error only)

pub(crate) struct UserLayer {
    color: bool,
}

impl UserLayer {
    pub(crate) fn new() -> Self {
        Self {
            color: should_color(),
        }
    }
}

impl<S: Subscriber> Layer<S> for UserLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = *meta.level();

        if level > Level::INFO {
            return;
        }

        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        let msg = visitor.0;

        let stderr = io::stderr();
        let mut handle = stderr.lock();

        if self.color {
            const ERROR_STYLE: Style = AnsiColor::BrightRed.on_default();
            const WARN_STYLE: Style = AnsiColor::Yellow.on_default();
            const RESET: anstyle::Reset = anstyle::Reset;

            match level {
                Level::ERROR => {
                    let _ = writeln!(handle, "{ERROR_STYLE}ERR {RESET}{msg}");
                }
                Level::WARN => {
                    let _ = writeln!(handle, "{WARN_STYLE}WARN {RESET}{msg}");
                }
                _ => {
                    let _ = writeln!(handle, "{msg}");
                }
            }
        } else {
            match level {
                Level::ERROR => {
                    let _ = writeln!(handle, "ERR {msg}");
                }
                Level::WARN => {
                    let _ = writeln!(handle, "WARN {msg}");
                }
                _ => {
                    let _ = writeln!(handle, "{msg}");
                }
            }
        }
    }
}

struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            use fmt::Write;
            let _ = write!(self.0, "{value:?}");
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}
