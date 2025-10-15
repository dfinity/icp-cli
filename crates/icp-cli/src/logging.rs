use std::{
    io::Write,
    os::fd::{AsRawFd, RawFd},
};

use tracing::debug;

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
