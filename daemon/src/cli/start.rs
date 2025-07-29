use anyhow::Result;
use log::info;
use std::env;
use std::process::Command;

pub async fn execute() -> Result<()> {
    info!("Starting warden daemon as a background service...");

    // Get the path to the current executable
    let current_exe = env::current_exe()?;

    // Start the daemon in the background using nohup
    let status = Command::new("nohup")
        .arg(current_exe)
        .arg("run")
        .arg(">") // Redirect stdout
        .arg("/tmp/warden.log")
        .arg("2>&1") // Redirect stderr to stdout
        .arg("&") // Run in background
        .status()?;

    if status.success() {
        info!("Daemon started successfully in the background");
    } else {
        info!("Failed to start daemon: {status:?}");
    }

    Ok(())
}
