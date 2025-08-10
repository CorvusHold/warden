use anyhow::{anyhow, Result};
use log::{debug, error, info};
use std::process::Command;

/// Start the Overwatch service
pub async fn start() -> Result<()> {
    info!("Starting Overwatch service");

    // Execute the system command to start the service
    let output = Command::new("systemctl")
        .args(["start", "warden-overwatch"])
        .output()
        .map_err(|e| anyhow!("Failed to execute start command: {}", e))?;

    if output.status.success() {
        info!("Overwatch service started successfully");
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        error!("Failed to start Overwatch service: {error}");
        Err(anyhow::anyhow!(
            "Failed to start Overwatch service: {}",
            error
        ))
    }
}

/// Stop the Overwatch service
pub async fn stop() -> Result<()> {
    info!("Stopping Overwatch service");

    // Execute the system command to stop the service
    let output = Command::new("systemctl")
        .args(["stop", "warden-overwatch"])
        .output()
        .map_err(|e| anyhow!("Failed to execute stop command: {}", e))?;

    if output.status.success() {
        info!("Overwatch service stopped successfully");
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        error!("Failed to stop Overwatch service: {error}");
        Err(anyhow::anyhow!(
            "Failed to stop Overwatch service: {}",
            error
        ))
    }
}

/// Restart the Overwatch service
pub async fn restart() -> Result<()> {
    info!("Restarting Overwatch service");

    // Execute the system command to restart the service
    let output = Command::new("systemctl")
        .args(["restart", "warden-overwatch"])
        .output()
        .map_err(|e| anyhow!("Failed to execute restart command: {}", e))?;

    if output.status.success() {
        info!("Overwatch service restarted successfully");
        Ok(())
    } else {
        let error = String::from_utf8_lossy(&output.stderr);
        error!("Failed to restart Overwatch service: {error}");
        Err(anyhow::anyhow!(
            "Failed to restart Overwatch service: {}",
            error
        ))
    }
}

/// Check if the Overwatch service is running
pub async fn is_running() -> Result<bool> {
    debug!("Checking if Overwatch service is running");

    // Execute the system command to check service status
    let output = Command::new("systemctl")
        .args(["is-active", "warden-overwatch"])
        .output()
        .map_err(|e| anyhow!("Failed to execute status check command: {}", e))?;

    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Ok(status == "active")
}
