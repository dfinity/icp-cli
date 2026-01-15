use tracing::{Level, Subscriber};
use tracing_subscriber::{
    Layer,
    filter::{Filtered, Targets},
    fmt::format,
    registry::LookupSpan,
};

type DebugLayer<S> = Filtered<
    tracing_subscriber::fmt::Layer<S, format::DefaultFields, format::Format<format::Full, ()>>,
    Targets,
    S,
>;

pub(crate) fn debug_layer<S: Subscriber + for<'a> LookupSpan<'a>>() -> DebugLayer<S> {
    let workspace_targets = Targets::new()
        .with_target("icp-cli", Level::DEBUG)
        .with_target("icp", Level::DEBUG);

    tracing_subscriber::fmt::layer()
        .without_time()
        .with_filter(workspace_targets)
}
