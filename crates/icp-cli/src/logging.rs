use tracing::{Level, Subscriber};
use tracing_subscriber::{
    Layer,
    filter::{Filtered, Targets},
    fmt::format,
    registry::LookupSpan,
};

type LoggingLayer<S> = Filtered<
    tracing_subscriber::fmt::Layer<S, format::DefaultFields, format::Format<format::Full, ()>>,
    Targets,
    S,
>;

/// Creates a logging layer configured based on whether debug mode is enabled
///
/// When is_debug is false (normal mode):
/// - Shows INFO, WARN, ERROR levels
/// - Hides level and target for clean output
///
/// When is_debug is true:
/// - Shows all levels including DEBUG
/// - Shows level and target for detailed debugging
///
/// Only targets workspace crates (icp-cli, icp) to skip printing dependencies' logs
pub fn logging_layer<S: Subscriber + for<'a> LookupSpan<'a>>(is_debug: bool) -> LoggingLayer<S> {
    let level = if is_debug { Level::DEBUG } else { Level::INFO };

    let workspace_targets = Targets::new()
        .with_target("icp-cli", level)
        .with_target("icp", level);

    tracing_subscriber::fmt::layer()
        .without_time()
        .with_level(is_debug)
        .with_target(is_debug)
        .with_filter(workspace_targets)
}
