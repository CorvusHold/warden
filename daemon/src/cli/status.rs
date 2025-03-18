use anyhow::Result;
use log::info;
use std::fs;
use std::path::Path;
use std::process::Command;

pub async fn execute() -> Result<()> {
    info!("Checking warden daemon status...");

    // Check if PID file exists
    let pid_file = "/tmp/warden.pid";
    if Path::new(pid_file).exists() {
        // Read PID from file
        let pid = fs::read_to_string(pid_file)?.trim().parse::<u32>()?;

        // Check if process is running
        let status = Command::new("ps")
            .arg("-p")
            .arg(pid.to_string())
            .arg("-o")
            .arg("pid=")
            .output()?;

        if !status.stdout.is_empty() {
            info!("Daemon is running with PID {}", pid);
        } else {
            info!("Daemon is not running (stale PID file found)");
            // Remove stale PID file
            fs::remove_file(pid_file)?;
        }
    } else {
        info!("Daemon is not running (no PID file found)");
    }

    Ok(())
}
