use anyhow::{Context, Result};
use log::{error, info};
use std::fs::File;
use std::io::Write;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal;

use crate::Daemon;

pub async fn execute() -> Result<()> {
    info!("Running warden daemon in the foreground...");

    // Create PID file
    let pid = process::id();
    let pid_file = "/tmp/warden.pid";
    let mut file =
        File::create(pid_file).context(format!("Failed to create PID file at {}", pid_file))?;
    write!(file, "{}", pid).context("Failed to write PID to file")?;

    info!("Created PID file at {} with PID {}", pid_file, pid);

    // Load configuration
    let config = match common::config::load_config() {
        Ok(config) => {
            info!("Configuration loaded successfully");

            // Log MQTT configuration if present
            if let Some(mqtt_config) = &config.mqtt {
                info!("MQTT broker: {}", mqtt_config.broker);
                info!("MQTT port: {}", mqtt_config.port.unwrap_or(1883));
                if let Some(topics) = &mqtt_config.topics {
                    if !topics.is_empty() {
                        info!("MQTT topics: {}", topics.join(", "));
                    }
                }
            } else {
                info!("No explicit MQTT configuration found, using defaults");
            }

            config
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e.into());
        }
    };

    // Create daemon instance
    let mut daemon = Daemon::new(config);

    // Initialize AMQP client
    if let Err(e) = daemon.init_amqp().await {
        error!("Failed to initialize AMQP client: {}", e);
        std::fs::remove_file(pid_file).ok(); // Clean up PID file on error
        return Err(e);
    }

    // Set up signal handling for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let pid_file_clone = pid_file.to_string();

    // Handle SIGINT (Ctrl+C)
    tokio::spawn(async move {
        if let Err(e) = signal::ctrl_c().await {
            error!("Failed to listen for Ctrl+C: {}", e);
            return;
        }

        info!("Received Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);

        // Remove PID file on Ctrl+C
        if let Err(e) = std::fs::remove_file(&pid_file_clone) {
            error!("Failed to remove PID file: {}", e);
        }
    });

    // Start the daemon
    info!("Daemon started, processing messages");

    // Run the daemon until a signal is received
    let result = daemon.start().await;

    if let Err(ref e) = result {
        error!("Daemon error: {}", e);
    }

    // Perform cleanup
    if let Err(e) = daemon.stop().await {
        error!("Error during daemon shutdown: {}", e);
    }

    // Remove PID file on normal exit
    if let Err(e) = std::fs::remove_file(pid_file) {
        // Don't error if file is already gone (might have been removed by signal handler)
        if e.kind() != std::io::ErrorKind::NotFound {
            error!("Failed to remove PID file: {}", e);
        }
    }

    info!("Daemon shutdown complete");
    result
}
