use std::process::{Child, Command};

pub struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    pub fn spawn(cmd: &mut Command) -> std::io::Result<Self> {
        let child = cmd.spawn()?;
        Ok(Self { child })
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {
                // Already exited, nothing to do
            }
            Ok(None) => {
                if let Err(e) = self.child.kill() {
                    eprintln!("Failed to kill child process: {}", e);
                }
                if let Err(e) = self.child.wait() {
                    eprintln!("Failed to wait on child process: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Failed to check child process status: {}", e);
            }
        }
    }
}
