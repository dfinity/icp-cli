use std::process::{Child, Command};

pub struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    pub fn spawn(cmd: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let child = cmd.spawn()?;
        Ok(Self { child })
    }

    fn send_sigint_to_process_group(&mut self) {
        #[cfg(unix)]
        {
            use nix::sys::signal::{Signal, killpg};
            use nix::unistd::Pid;

            let pid = self.child.id();
            let pgid = Pid::from_raw(pid as i32); // Child PID = PGID
            let _ = killpg(pgid, Signal::SIGINT);
            // Give the process some time to shut down gracefully
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {
                // Already exited, nothing to do
            }
            Ok(None) => {
                self.send_sigint_to_process_group();
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
