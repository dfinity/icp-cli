use tracing::{Level, Subscriber};
use tracing_subscriber::{
    Layer,
    filter::{Filtered, Targets},
    fmt,
    registry::LookupSpan,
};

type DebugLayer<S> = Filtered<
    fmt::Layer<S, fmt::format::DefaultFields, fmt::format::Format<fmt::format::Full, ()>>,
    Targets,
    S,
>;

pub fn debug_layer<S: Subscriber + for<'a> LookupSpan<'a>>() -> DebugLayer<S> {
    // Only target the workspace crates and avoid printing debug logs from dependencies
    let workspace_targets = Targets::new()
        .with_target("icp-cli", Level::DEBUG)
        .with_target("icp", Level::DEBUG);

    tracing_subscriber::fmt::layer()
        .without_time()
        .with_target(true)
        .with_filter(workspace_targets)
}
