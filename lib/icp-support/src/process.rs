use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, Signal, System};

pub fn process_running(pid: u32) -> bool {
    let pid = Pid::from_u32(pid);

    let system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()),
    );

    system.process(pid).is_some()
}
