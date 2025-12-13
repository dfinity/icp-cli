use std::io::Write;
#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, RawHandle};

use tracing::{Level, Subscriber, debug};
use tracing_subscriber::{
    Layer,
    filter::{Filtered, Targets},
    fmt::format,
    registry::LookupSpan,
};

#[expect(unused)]
#[derive(Debug)]
pub(crate) struct TermWriter<W> {
    pub(crate) debug: bool,
    pub(crate) writer: Box<W>,
}

impl<W: Write> Write for TermWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if !self.debug {
            self.writer.write(buf)?;
        }
        debug!("{}", String::from_utf8_lossy(buf).trim());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.debug {
            self.writer.flush()?;
        }
        Ok(())
    }
}

#[cfg(unix)]
impl<W: AsRawFd> AsRawFd for TermWriter<W> {
    fn as_raw_fd(&self) -> RawFd {
        self.writer.as_raw_fd()
    }
}
#[cfg(windows)]
impl<W: AsRawHandle> AsRawHandle for TermWriter<W> {
    fn as_raw_handle(&self) -> RawHandle {
        self.writer.as_raw_handle()
    }
}

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
