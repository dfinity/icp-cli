use std::path::Path;

pub fn spawn_pocketic(pocketic_path: &Path, port_file: &Path) -> tokio::process::Child {
    let mut cmd = tokio::process::Command::new(pocketic_path);
    cmd.arg("--port-file");
    cmd.arg(port_file.as_os_str());
    cmd.args(["--ttl", "2592000", "--log-levels", "error"]);

    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());
    #[cfg(unix)]
    {
        //use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    eprintln!("Starting PocketIC...");
    cmd.spawn().expect("Could not start PocketIC.")
}
