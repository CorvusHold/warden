use anyhow::{Context, Result};
use log::{error, info};
use std::fs::File;
use std::io::Write;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal;

use crate::scheduler::{SchedulerEvent, SchedulerOptions};
use crate::Daemon;

pub async fn execute() -> Result<()> {
    info!("Running warden daemon in the foreground...");

    // Create PID file
    let pid = process::id();
    let pid_file = "/tmp/warden.pid";
    let mut file =
        File::create(pid_file).context(format!("Failed to create PID file at {pid_file}"))?;
    write!(file, "{pid}").context("Failed to write PID to file")?;

    info!("Created PID file at {pid_file} with PID {pid}");

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
            error!("Failed to load configuration: {e}");
            return Err(e.into());
        }
    };

    // Create daemon instance
    let mut daemon = Daemon::new(config);

    // Initialize AMQP client
    if let Err(e) = daemon.init_amqp().await {
        error!("Failed to initialize AMQP client: {e}");
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
            error!("Failed to listen for Ctrl+C: {e}");
            return;
        }

        info!("Received Ctrl+C, shutting down...");
        r.store(false, Ordering::SeqCst);

        // Remove PID file on Ctrl+C
        if let Err(e) = std::fs::remove_file(&pid_file_clone) {
            error!("Failed to remove PID file: {e}");
        }
    });

    // Start the daemon
    info!("Daemon started, processing messages");

    // Check if schedules are configured and start the scheduler
    let config_for_scheduler = daemon.config();
    let has_schedules = {
        match config_for_scheduler.lock() {
            Ok(cfg) => {
                cfg.schedules.as_ref().map(|s| !s.backups.is_empty() || !s.retention.is_empty()).unwrap_or(false)
            }
            Err(poisoned) => {
                // Mutex was poisoned (another thread panicked while holding it)
                // Recover the data and continue - schedules are non-critical for startup
                error!("Config mutex was poisoned, recovering: {}", poisoned);
                let cfg = poisoned.into_inner();
                cfg.schedules.as_ref().map(|s| !s.backups.is_empty() || !s.retention.is_empty()).unwrap_or(false)
            }
        }
    };

    // Create scheduler event channel
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<SchedulerEvent>(100);

    // Spawn scheduler event handler (track handle for clean shutdown)
    let event_handler = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                SchedulerEvent::TaskStarted { schedule_id, schedule_type, started_at } => {
                    info!("[Scheduler] Task started: {} ({}) at {}", schedule_id, schedule_type, started_at);
                }
                SchedulerEvent::TaskCompleted(result) => {
                    if result.success {
                        info!(
                            "[Scheduler] Task completed: {} ({}) - {}",
                            result.schedule_id,
                            result.schedule_type,
                            result.message.as_deref().unwrap_or("success")
                        );
                        if let Some(backup_id) = result.backup_id {
                            info!("[Scheduler] Backup ID: {}", backup_id);
                        }
                    } else {
                        error!(
                            "[Scheduler] Task failed: {} ({}) - {}",
                            result.schedule_id,
                            result.schedule_type,
                            result.message.as_deref().unwrap_or("unknown error")
                        );
                    }
                }
                SchedulerEvent::Error { schedule_id, message } => {
                    error!(
                        "[Scheduler] Error{}: {}",
                        schedule_id.map(|id| format!(" ({})", id)).unwrap_or_default(),
                        message
                    );
                }
            }
        }
    });

    // Spawn scheduler if schedules are configured
    let scheduler_handle = if has_schedules {
        info!("Schedules configured, starting scheduler...");
        let scheduler_config = config_for_scheduler.clone();
        let scheduler_options = SchedulerOptions::default();
        
        Some(tokio::spawn(async move {
            if let Err(e) = crate::scheduler::run_scheduler(
                scheduler_config,
                scheduler_options,
                Some(event_tx),
            ).await {
                error!("[Scheduler] Scheduler error: {}", e);
            }
        }))
    } else {
        info!("No schedules configured, scheduler not started");
        drop(event_tx); // Allow event handler task to exit
        None
    };

    // Run the daemon until a signal is received
    let result = daemon.start().await;

    if let Err(ref e) = result {
        error!("Daemon error: {e}");
    }

    // Abort scheduler if running and wait for clean shutdown
    if let Some(handle) = scheduler_handle {
        handle.abort();
        // Wait for task to fully stop (abort error is expected)
        let _ = handle.await;
        info!("Scheduler stopped");
    }

    // Wait for event handler to finish processing remaining events
    // The channel will close when scheduler/event_tx is dropped, causing event_handler to exit
    let _ = event_handler.await;
    info!("Event handler stopped");

    // Perform cleanup
    if let Err(e) = daemon.stop().await {
        error!("Error during daemon shutdown: {e}");
    }

    // Remove PID file on normal exit
    if let Err(e) = std::fs::remove_file(pid_file) {
        // Don't error if file is already gone (might have been removed by signal handler)
        if e.kind() != std::io::ErrorKind::NotFound {
            error!("Failed to remove PID file: {e}");
        }
    }

    info!("Daemon shutdown complete");
    result
}
