use anyhow::Result;
use log::info;
use std::fs;
use std::path::Path;
use std::process::Command;

pub async fn execute() -> Result<()> {
    info!("Stopping warden daemon...");

    // Check if PID file exists
    let pid_file = "/tmp/warden.pid";
    if Path::new(pid_file).exists() {
        // Read PID from file
        let pid = fs::read_to_string(pid_file)?.trim().parse::<u32>()?;

        // Send SIGTERM to the process
        let status = Command::new("kill")
            .arg("-15") // SIGTERM
            .arg(pid.to_string())
            .status()?;

        if status.success() {
            info!("Daemon stopped successfully");
            // Remove PID file
            fs::remove_file(pid_file)?;
        } else {
            info!("Failed to stop daemon: {status:?}");
        }
    } else {
        info!("No PID file found, daemon may not be running");
    }

    Ok(())
}
