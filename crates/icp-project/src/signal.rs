/// Utilities for handling process termination signals across platforms.
#[cfg(feature = "host")]
use tokio::select;

/// Waits for a stop signal (Ctrl+C, SIGTERM on Unix, or window close on Windows).
///
/// This function provides cross-platform signal handling:
/// - Unix: Listens for SIGINT (Ctrl+C) and SIGTERM (graceful shutdown)
/// - Windows: Listens for Ctrl+C, Ctrl+Break, and window close events
///
/// # Examples
///
/// ```no_run
/// use icp_project::signal::stop_signal;
/// use tokio::select;
///
/// # async fn example() {
/// loop {
///     select! {
///         _ = do_work() => { }
///         _ = stop_signal() => {
///             println!("Received stop signal, shutting down...");
///             break;
///         }
///     }
/// }
/// # }
/// # async fn do_work() {}
/// ```
#[cfg(all(unix, feature = "host"))]
pub async fn stop_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigterm = signal(SignalKind::terminate()).unwrap();
    select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
}

/// Waits for a stop signal (Ctrl+C, SIGTERM on Unix, or window close on Windows).
///
/// This function provides cross-platform signal handling:
/// - Unix: Listens for SIGINT (Ctrl+C) and SIGTERM (graceful shutdown)
/// - Windows: Listens for Ctrl+C, Ctrl+Break, and window close events
///
/// # Examples
///
/// ```no_run
/// use icp_project::signal::stop_signal;
/// use tokio::select;
///
/// # async fn example() {
/// loop {
///     select! {
///         _ = do_work() => { }
///         _ = stop_signal() => {
///             println!("Received stop signal, shutting down...");
///             break;
///         }
///     }
/// }
/// # }
/// # async fn do_work() {}
/// ```
#[cfg(all(windows, feature = "host"))]
pub async fn stop_signal() {
    use tokio::signal::windows::{ctrl_break, ctrl_close};
    let mut ctrl_break = ctrl_break().unwrap();
    let mut ctrl_close = ctrl_close().unwrap();
    select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = ctrl_break.recv() => {},
        _ = ctrl_close.recv() => {},
    }
}

/// Waits for a stop signal — forever, where there is no process to signal.
///
/// A caller that races this against its own work is asking to be interrupted
/// if anything interrupts the program. Nothing can, off a host, so the arm
/// simply never fires.
#[cfg(not(feature = "host"))]
pub async fn stop_signal() {
    std::future::pending().await
}
