use async_dropper::{AsyncDrop, AsyncDropper};
use async_trait::async_trait;
use candid::Principal;
use notify::{EventHandler, Watcher};
use serde::Deserialize;
use snafu::prelude::*;
use std::{io::ErrorKind, process::Stdio, time::Duration};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};
use tokio::{
    process::Child,
    select,
    sync::mpsc::{Receiver, Sender},
    time::Instant,
};
use tracing::{info, warn};

use crate::{
    network::{ManagedLauncherConfig, Port, config::ChildLocator},
    prelude::*,
};

pub struct NetworkInstance {
    pub gateway_port: u16,
    pub root_key: Vec<u8>,
    pub pocketic_config_port: Option<u16>,
    pub pocketic_instance_id: Option<usize>,
    pub use_friendly_domains: bool,
}

#[derive(Debug, Snafu)]
pub enum SpawnNetworkLauncherError {
    #[snafu(display("failed to create stdio log at {path}"))]
    CreateStdioFile {
        source: std::io::Error,
        path: PathBuf,
    },
    #[snafu(display("failed to watch status directory"))]
    WatchStatusDir { source: WaitForFileError },
    #[snafu(display("failed to spawn network launcher {network_launcher_path}"))]
    SpawnLauncher {
        source: std::io::Error,
        network_launcher_path: PathBuf,
    },
    #[snafu(display("failed to watch launcher status file"))]
    WatchForStatusFile { source: WaitForLauncherStatusError },
    #[snafu(display(
        "network launcher at {network_launcher_path} exited prematurely with status {exit_status}{detail}"
    ))]
    LauncherExitedPrematurely {
        network_launcher_path: PathBuf,
        exit_status: std::process::ExitStatus,
        /// Either empty, or a leading-newline suffix carrying the launcher's captured
        /// stderr tail (background mode only). See [`premature_exit_detail`].
        detail: String,
    },
    #[snafu(display("failed to watch launcher process for exit code"))]
    WatchLauncher {
        network_launcher_path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to parse root key {key}"))]
    ParseRootKey {
        key: String,
        source: hex::FromHexError,
    },
}

pub async fn spawn_network_launcher(
    network_launcher_path: &Path,
    stdout_file: &Path,
    stderr_file: &Path,
    background: bool,
    verbose: bool,
    launcher_config: &ManagedLauncherConfig,
    state_dir: &Path,
    status_dir: &Path,
) -> Result<
    (
        AsyncDropper<ChildSignalOnDrop>,
        NetworkInstance,
        ChildLocator,
    ),
    SpawnNetworkLauncherError,
> {
    let mut cmd = tokio::process::Command::new(network_launcher_path);
    cmd.args([
        "--interface-version",
        "1.1.0",
        "--state-dir",
        state_dir.as_str(),
    ]);
    cmd.args(["--bind", &launcher_config.gateway.bind]);
    if let Port::Fixed(port) = launcher_config.gateway.port {
        cmd.args(["--gateway-port", &port.to_string()]);
    }
    cmd.args(["--status-dir", status_dir.as_str()]);
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.args(launcher_settings_flags(launcher_config));
    if background {
        info!("For background mode, network output will be redirected:");
        info!("  stdout: {stdout_file}");
        info!("  stderr: {stderr_file}");
        let stdout = std::fs::File::create(stdout_file)
            .context(CreateStdioFileSnafu { path: &stdout_file })?;
        let stderr = std::fs::File::create(stderr_file)
            .context(CreateStdioFileSnafu { path: &stderr_file })?;
        cmd.stdout(Stdio::from(stdout));
        cmd.stderr(Stdio::from(stderr));
    } else {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }
    let watcher = wait_for_launcher_status(status_dir).context(WatchStatusDirSnafu)?;
    let child = cmd.spawn().context(SpawnLauncherSnafu {
        network_launcher_path,
    })?;
    let mut guard = AsyncDropper::new(ChildSignalOnDrop { child: Some(child) });
    let child = guard.child.as_mut().unwrap();
    let launcher_status = select! {
        status = watcher => status.context(WatchForStatusFileSnafu)?,
        // If the child process exits before writing the status file, return an error.
        res = child.wait() => {
            let exit_status = res.context(WatchLauncherSnafu {
                network_launcher_path,
            })?;
            // In background mode the launcher's stderr was redirected to a file rather than
            // inherited, so nothing was shown live. Read it back so the error explains *why*
            // it exited (e.g. a port conflict) instead of just reporting the exit status.
            let detail = premature_exit_detail(background, stderr_file);
            return LauncherExitedPrematurelySnafu {
                exit_status,
                network_launcher_path,
                detail,
            }.fail();
        },
    };
    let pid = child.id().unwrap();
    // Get the process start time for uniqueness detection (handles PID reuse)
    let start_time = {
        let sysinfo_pid = Pid::from_u32(pid);
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::Some(&[sysinfo_pid]), true);
        system
            .process(sysinfo_pid)
            .map(|p| p.start_time())
            .unwrap_or(0)
    };
    Ok((
        guard,
        NetworkInstance {
            gateway_port: launcher_status.gateway_port,
            root_key: hex::decode(&launcher_status.root_key).context(ParseRootKeySnafu {
                key: &launcher_status.root_key,
            })?,
            pocketic_config_port: launcher_status.config_port,
            pocketic_instance_id: launcher_status.instance_id,
            use_friendly_domains: launcher_status
                .supported_features
                .iter()
                .any(|f| f == CUSTOM_DOMAINS_FEATURE),
        },
        ChildLocator::Pid { pid, start_time },
    ))
}

/// Upper bounds on how much captured output to fold into a premature-exit error, so a
/// verbose launcher log (e.g. `--debug` with `-d`) can't produce a wall-of-text error.
///
/// Shared with the Docker path, which applies the same bounds to the container log tail.
pub(super) const MAX_OUTPUT_TAIL_LINES: usize = 50;
pub(super) const MAX_OUTPUT_TAIL_BYTES: usize = 8 * 1024;

/// Builds the trailing detail appended to [`SpawnNetworkLauncherError::LauncherExitedPrematurely`].
///
/// In foreground mode the launcher's stderr is inherited and was already shown live, so this
/// returns an empty string. In background mode it was redirected to `stderr_file`; we read the
/// tail back so the cause travels with the error. The read is best-effort: if it fails or the
/// file is empty we fall back to pointing at the log path rather than masking the exit status.
fn premature_exit_detail(background: bool, stderr_file: &Path) -> String {
    if !background {
        return String::new();
    }
    match crate::fs::read_to_string(stderr_file) {
        Ok(contents) => {
            let tail = output_tail(&contents);
            if tail.is_empty() {
                format!("\nSee the launcher log at {stderr_file} for details.")
            } else {
                format!("\nLauncher error output (from {stderr_file}):\n{tail}")
            }
        }
        // The read error is deliberately ignored: this is already an error path, and the
        // fallback carries `stderr_file` so the user can still find the log.
        Err(_) => format!("\nSee the launcher log at {stderr_file} for details."),
    }
}

/// Returns the trailing portion of `contents`, capped to the last [`MAX_OUTPUT_TAIL_LINES`]
/// lines and then to [`MAX_OUTPUT_TAIL_BYTES`] (cutting on a char boundary), trimmed.
pub(super) fn output_tail(contents: &str) -> String {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(MAX_OUTPUT_TAIL_LINES);
    let by_lines = lines[start..].join("\n");
    let by_lines = by_lines.trim();
    tail_bytes(by_lines, MAX_OUTPUT_TAIL_BYTES)
        .trim_start()
        .to_string()
}

/// Returns at most the trailing `max_bytes` of `contents`, cutting on a char boundary.
///
/// Split out of [`output_tail`] so a caller streaming output in can hold a bounded buffer
/// without duplicating the boundary handling.
fn tail_bytes(contents: &str, max_bytes: usize) -> &str {
    if contents.len() <= max_bytes {
        return contents;
    }
    let mut cut = contents.len() - max_bytes;
    while !contents.is_char_boundary(cut) {
        cut += 1;
    }
    &contents[cut..]
}

pub async fn stop_launcher(pid: Pid) {
    send_sigint(pid);
    let mut system = System::new();
    let expire = Instant::now() + Duration::from_secs(10);
    loop {
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        match system.process(pid) {
            None => break,
            Some(_) => {
                if Instant::now() >= expire {
                    warn!("process {pid} did not exit within 10 seconds");
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub fn launcher_settings_flags(config: &ManagedLauncherConfig) -> Vec<String> {
    let ManagedLauncherConfig {
        gateway,
        version: _,
        artificial_delay_ms,
        ii,
        nns,
        subnets,
        bitcoind_addr,
        dogecoind_addr,
    } = config;
    let mut flags = vec![];
    if *ii {
        flags.push("--ii".to_string());
    }
    if *nns {
        flags.push("--nns".to_string());
    }
    if let Some(delay) = artificial_delay_ms {
        flags.push(format!("--artificial-delay-ms={delay}"));
    }
    if let Some(subnets) = &subnets {
        for subnet in subnets {
            flags.push(format!("--subnet={subnet}"));
        }
    }
    if let Some(addrs) = &bitcoind_addr {
        for addr in addrs {
            flags.push(format!("--bitcoind-addr={addr}"));
        }
    }
    if let Some(addrs) = &dogecoind_addr {
        for addr in addrs {
            flags.push(format!("--dogecoind-addr={addr}"));
        }
    }
    for domain in &gateway.domains {
        flags.push(format!("--domain={domain}"));
    }
    if gateway.domains.is_empty() {
        flags.push(format!("--domain={}", gateway.bind));
    }
    flags
}

pub fn send_sigint(pid: Pid) {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    if let Some(process) = system.process(pid) {
        process.kill_with(Signal::Interrupt);
    }
}

#[derive(Default)]
pub struct ChildSignalOnDrop {
    pub child: Option<Child>,
}

impl ChildSignalOnDrop {
    pub async fn signal_and_wait(&mut self) -> std::io::Result<()> {
        if let Some(mut child) = self.child.take()
            && let Some(id) = child.id()
        {
            send_sigint((id as usize).into());
            child.wait().await?;
        }
        Ok(())
    }
    pub fn defuse(&mut self) {
        self.child = None;
    }
}

#[async_trait]
impl AsyncDrop for ChildSignalOnDrop {
    async fn async_drop(&mut self) {
        _ = self.signal_and_wait().await;
    }
}

#[derive(Debug, Snafu)]
pub enum WaitForFileError {
    #[snafu(display("failed to watch file changes at path {path}"))]
    Watch {
        source: notify::Error,
        path: PathBuf,
    },

    #[snafu(display("failed to read event for file {path}"))]
    ReadEvent {
        source: notify::Error,
        path: PathBuf,
    },

    #[snafu(transparent)]
    ReadFile { source: crate::fs::IoError },
}

/// Waits for a file to be created and have a full line of content. Call the function before initing the external process,
/// then await the future after the init.
pub fn wait_for_single_line_file(
    path: &Path,
) -> Result<impl Future<Output = Result<String, WaitForFileError>> + use<>, WaitForFileError> {
    let dir = path.parent().unwrap();
    // notify will get here faster
    let (rec_tx, rec_rx) = tokio::sync::mpsc::channel(10);
    let mut rec_watcher =
        notify::recommended_watcher(WatchRecv(rec_tx)).context(WatchSnafu { path: &dir })?;
    // poll is more reliable when dealing with vfs like 9p, notably in WSL2
    let (poll_tx, poll_rx) = tokio::sync::mpsc::channel(10);
    let poll_watcher = notify::PollWatcher::new(
        WatchRecv(poll_tx),
        notify::Config::default()
            .with_poll_interval(Duration::from_millis(100))
            .with_compare_contents(true),
    )
    .context(WatchSnafu { path: &dir })?;
    // Assembled before either watcher is registered, so that every exit from here on unwinds
    // through the session's field order rather than these locals'.
    let mut session = WatchSession {
        rec_rx,
        poll_rx,
        rec_watcher,
        poll_watcher,
    };
    session
        .rec_watcher
        .watch(dir.as_std_path(), notify::RecursiveMode::NonRecursive)
        .context(WatchSnafu { path: &dir })?;
    session
        .poll_watcher
        .watch(dir.as_std_path(), notify::RecursiveMode::NonRecursive)
        .context(WatchSnafu { path: &dir })?;
    _ = session.poll_watcher.poll();
    let path = path.to_path_buf();
    let dir = dir.to_path_buf();
    Ok(async move {
        loop {
            let evt = session.next_event().await;
            let Some(res) = evt else {
                unreachable!("watcher dropped while waiting for file");
            };
            let event = res.context(ReadEventSnafu { path: &dir })?;
            if event.kind.is_modify() || event.kind.is_create() {
                match crate::fs::read_to_string(&path) {
                    Ok(content) => {
                        if content.ends_with('\n') {
                            return Ok(content);
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
    })
}

/// Call the function before initing the external process, then await the future after the init.
pub fn wait_for_launcher_status(
    status_dir: &Path,
) -> Result<
    impl Future<Output = Result<LauncherStatus, WaitForLauncherStatusError>> + use<>,
    WaitForFileError,
> {
    let status_file = status_dir.join("status.json");
    let watcher = wait_for_single_line_file(&status_file)?;
    Ok(async move {
        let status_content = watcher.await.context(WaitForFileSnafu)?;
        let launcher_status: LauncherStatus =
            serde_json::from_str(&status_content).context(DeserializeSnafu)?;
        ensure!(
            launcher_status.v == "1",
            BadVersionSnafu {
                expected: "1",
                found: &launcher_status.v
            }
        );
        Ok(launcher_status)
    })
}

#[derive(Debug, Snafu)]
pub enum WaitForLauncherStatusError {
    WaitForFile { source: WaitForFileError },
    Deserialize { source: serde_json::Error },
    BadVersion { expected: String, found: String },
}

#[derive(Deserialize)]
pub struct LauncherStatus {
    pub v: String,
    pub instance_id: Option<usize>,
    pub config_port: Option<u16>,
    pub gateway_port: u16,
    pub root_key: String,
    pub default_effective_canister_id: Option<Principal>,
    #[serde(default)]
    pub supported_features: Vec<String>,
}

pub const CUSTOM_DOMAINS_FEATURE: &str = "custom-domains";

/// Keeps each watcher together with its receiver, relying on struct fields being dropped in
/// declaration order so the receivers always go first.
///
/// Both watchers hand events over with [`Sender::blocking_send`], and both stop by waiting for
/// their callback thread to go idle - the FSEvents backend does so by busy-waiting. A callback
/// parked on a full channel would therefore make that wait spin forever; closing the channels
/// first releases it. Grouping the four into one value keeps that order whether the future is
/// dropped part-way through or before it is ever polled.
struct WatchSession {
    rec_rx: Receiver<notify::Result<notify::Event>>,
    poll_rx: Receiver<notify::Result<notify::Event>>,
    rec_watcher: notify::RecommendedWatcher,
    poll_watcher: notify::PollWatcher,
}

impl WatchSession {
    /// Takes `&mut self` rather than the receivers, so that awaiting this keeps the whole session
    /// - watchers included - alive.
    async fn next_event(&mut self) -> Option<notify::Result<notify::Event>> {
        select! {
            rec = self.rec_rx.recv() => rec,
            poll = self.poll_rx.recv() => poll,
        }
    }
}

struct WatchRecv(Sender<notify::Result<notify::Event>>);

impl EventHandler for WatchRecv {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        let _ = self.0.blocking_send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_once_the_file_has_a_full_line() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let file = dir.path().join("status.json");
        let fut = wait_for_single_line_file(&file).unwrap();

        std::thread::spawn({
            let file = file.clone();
            move || {
                std::thread::sleep(Duration::from_millis(200));
                std::fs::write(&file, b"partial").unwrap();
                std::thread::sleep(Duration::from_millis(200));
                std::fs::write(&file, b"complete\n").unwrap();
            }
        });

        let content = tokio::time::timeout(Duration::from_secs(20), fut)
            .await
            .expect("timed out waiting for the file")
            .unwrap();
        assert_eq!(content, "complete\n");
    }

    /// Saturates the channels without ever polling the future, then drops it. If the receivers
    /// were not dropped ahead of the watchers, the callback thread would still be parked in
    /// `blocking_send` and the watchers' shutdown would never finish.
    #[test]
    fn dropping_an_unpolled_watcher_does_not_wedge() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let dir = dir.path().to_path_buf();
        let fut = wait_for_single_line_file(&dir.join("status.json")).unwrap();

        for i in 0..500 {
            std::fs::write(dir.join(format!("churn{i}")), b"x").unwrap();
        }
        std::thread::sleep(Duration::from_millis(1500));

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(fut);
            let _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(20))
            .expect("dropping the future wedged");
    }

    #[test]
    fn output_tail_keeps_short_content_verbatim() {
        assert_eq!(output_tail("line one\nline two"), "line one\nline two");
    }

    #[test]
    fn output_tail_trims_surrounding_whitespace() {
        assert_eq!(output_tail("\n\n  boom  \n\n"), "boom");
    }

    #[test]
    fn output_tail_keeps_only_last_lines() {
        let input = (0..200)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let tail = output_tail(&input);
        let lines: Vec<&str> = tail.lines().collect();
        assert_eq!(lines.len(), MAX_OUTPUT_TAIL_LINES);
        assert_eq!(*lines.first().unwrap(), "150");
        assert_eq!(*lines.last().unwrap(), "199");
    }

    #[test]
    fn output_tail_caps_bytes_on_a_single_long_line() {
        let input = "x".repeat(MAX_OUTPUT_TAIL_BYTES * 2);
        let tail = output_tail(&input);
        assert!(tail.len() <= MAX_OUTPUT_TAIL_BYTES);
        assert!(!tail.is_empty());
    }

    #[test]
    fn tail_bytes_returns_everything_within_the_limit() {
        assert_eq!(tail_bytes("line one\nline two", 1024), "line one\nline two");
    }

    #[test]
    fn tail_bytes_cuts_on_a_char_boundary() {
        // Each 'é' is two bytes, so cutting at a raw byte offset can land mid-character — which
        // would panic when slicing rather than merely truncate.
        let input = "é".repeat(8);
        let tail = tail_bytes(&input, 5);
        assert!(tail.len() <= 5, "{}", tail.len());
        assert!(tail.chars().all(|c| c == 'é'), "{tail}");
    }

    #[test]
    fn premature_exit_detail_is_empty_in_foreground() {
        // Foreground inherits stderr, so there is nothing to read back.
        let missing = Path::new("/nonexistent/stderr.log");
        assert_eq!(premature_exit_detail(false, missing), "");
    }

    #[test]
    fn premature_exit_detail_includes_captured_output() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let file = dir.path().join("stderr.log");
        crate::fs::write(&file, b"Address already in use (os error 48)\n").unwrap();
        let detail = premature_exit_detail(true, &file);
        assert!(detail.starts_with('\n'));
        assert!(detail.contains("Address already in use"));
        assert!(detail.contains(file.as_str()));
    }

    #[test]
    fn premature_exit_detail_falls_back_to_log_path_when_unreadable() {
        let missing = Path::new("/nonexistent/stderr.log");
        let detail = premature_exit_detail(true, missing);
        assert!(detail.contains("See the launcher log at"));
        assert!(detail.contains(missing.as_str()));
    }
}
