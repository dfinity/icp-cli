use std::{
    io::Write,
    os::fd::{AsRawFd, RawFd},
};

use tracing::{Level, Subscriber, debug};
use tracing_subscriber::{
    Layer,
    filter::{Filtered, Targets},
    fmt::format,
    registry::LookupSpan,
};

#[derive(Debug)]
pub struct TermWriter<W: Write + AsRawFd> {
    pub debug: bool,
    pub writer: Box<W>,
}

impl<W: Write + AsRawFd> Write for TermWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if !self.debug {
            self.writer.write(buf)?;
        }
        debug!("{}", String::from_utf8_lossy(buf));
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.debug {
            self.writer.flush()?;
        }
        Ok(())
    }
}

impl<W: Write + AsRawFd> AsRawFd for TermWriter<W> {
    fn as_raw_fd(&self) -> RawFd {
        self.writer.as_raw_fd()
    }
}

type DebugLayer<S> = Filtered<
    tracing_subscriber::fmt::Layer<S, format::DefaultFields, format::Format<format::Full, ()>>,
    Targets,
    S,
>;

pub fn debug_layer<S: Subscriber + for<'a> LookupSpan<'a>>() -> DebugLayer<S> {
    let workspace_targets = Targets::new()
        .with_target("icp-cli", Level::DEBUG)
        .with_target("icp", Level::DEBUG);

    tracing_subscriber::fmt::layer()
        .without_time()
        .with_filter(workspace_targets)
}
