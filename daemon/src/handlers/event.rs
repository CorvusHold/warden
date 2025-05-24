use crate::amqp::{AmqpClient, MessageType};
use anyhow::{anyhow, Result};
use common::config::WardenConfig;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Helper function to extract subtopic from routing key
fn get_subtopic(routing_key: &str, prefix: &str) -> Option<String> {
    routing_key
        .strip_prefix(prefix)
        .map(|stripped| stripped.to_string())
}

/// Event types that the daemon can handle
#[derive(Debug, Serialize, Deserialize)]
pub enum EventType {
    PostgresAlert,
    OverwatchAlert,
    SystemAlert,
    Custom(String),
}

/// Event payload structure
#[derive(Debug, Serialize, Deserialize)]
pub struct EventPayload {
    pub event_type: EventType,
    pub severity: EventSeverity,
    pub source: String,
    pub message: String,
    pub data: Option<HashMap<String, serde_json::Value>>,
}

/// Event severity levels
#[derive(Debug, Serialize, Deserialize)]
pub enum EventSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl std::fmt::Display for EventSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventSeverity::Info => write!(f, "Info"),
            EventSeverity::Warning => write!(f, "Warning"),
            EventSeverity::Error => write!(f, "Error"),
            EventSeverity::Critical => write!(f, "Critical"),
        }
    }
}

/// Handle event messages
pub async fn handle_event(
    routing_key: &str,
    payload: &str,
    client: &Arc<AmqpClient>,
    config: &Arc<Mutex<WardenConfig>>,
) -> Result<()> {
    // Extract event subtopic from routing key
    let subtopic = get_subtopic(routing_key, "warden.events.")
        .ok_or_else(|| anyhow!("Invalid event routing key format"))?;

    // Parse event payload
    let event = match serde_json::from_str::<EventPayload>(payload) {
        Ok(evt) => evt,
        Err(e) => {
            let error_msg = format!("Failed to parse event payload: {}", e);
            error!("{}", error_msg);
            return Err(anyhow!(error_msg));
        }
    };

    info!(
        "Received event: {:?} from {}",
        event.event_type, event.source
    );
    debug!("Event details: {:?}", event);

    // Get exchange name from config
    let exchange = {
        let config_guard = config.lock().unwrap();
        if let Some(mqtt_config) = &config_guard.mqtt {
            mqtt_config
                .exchange
                .clone()
                .unwrap_or_else(|| "warden".to_string())
        } else {
            "warden".to_string()
        }
    };

    // Process event based on type
    match event.event_type {
        EventType::PostgresAlert => {
            // Check if PostgreSQL backup feature is enabled
            if !config.lock().unwrap().features.postgres_backup {
                warn!("PostgreSQL event received but feature is not enabled");
                return Ok(());
            }

            // Process PostgreSQL alert
            match handle_postgres_event(&event, client).await {
                Ok(_) => info!("PostgreSQL event handled successfully"),
                Err(e) => error!("Failed to handle PostgreSQL event: {}", e),
            }
        }
        EventType::OverwatchAlert => {
            // Check if Overwatch feature is enabled
            if !config.lock().unwrap().features.overwatch {
                warn!("Overwatch event received but feature is not enabled");
                return Ok(());
            }

            // Process Overwatch alert
            match handle_overwatch_event(&event, client).await {
                Ok(_) => info!("Overwatch event handled successfully"),
                Err(e) => error!("Failed to handle Overwatch event: {}", e),
            }
        }
        EventType::SystemAlert => {
            // Process system alert
            match handle_system_event(&event, client).await {
                Ok(_) => info!("System event handled successfully"),
                Err(e) => error!("Failed to handle system event: {}", e),
            }
        }
        EventType::Custom(event_name) => {
            // Handle custom event
            warn!("Received custom event: {}", event_name);
            // For now, we just log custom events
        }
    }

    // Acknowledge event receipt
    let ack_routing_key = format!("warden.events.{}.ack", subtopic);
    let ack_payload = serde_json::json!({
        "event_id": event.data.as_ref().and_then(|d| d.get("id")).unwrap_or(&serde_json::Value::Null),
        "status": "received",
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });

    match client
        .publish(
            &exchange,
            &ack_routing_key,
            MessageType::Acknowledgment,
            &serde_json::to_string(&ack_payload)?,
        )
        .await
    {
        Ok(_) => debug!("Event acknowledgment sent"),
        Err(e) => error!("Failed to send event acknowledgment: {}", e),
    }

    Ok(())
}

/// Handle PostgreSQL events
async fn handle_postgres_event(event: &EventPayload, _client: &Arc<AmqpClient>) -> Result<()> {
    match event.severity {
        EventSeverity::Critical => {
            // For critical PostgreSQL events, we might want to take immediate action
            // like creating an emergency backup
            if let Some(data) = &event.data {
                if let Some(db_name) = data.get("database").and_then(|v| v.as_str()) {
                    info!("Creating emergency backup for database: {}", db_name);

                    // Call PostgreSQL backup functionality
                    //     match postgres::backup::create_backup().await {
                    //         Ok(backup_info) => {
                    //             info!("Emergency backup created successfully");

                    //             // Notify about the backup
                    //             let routing_key = "warden.notifications.postgres";
                    //             let response_payload = serde_json::json!({
                    //                 "action": "emergency_backup",
                    //                 "status": "success",
                    //                 "database": db_name,
                    //                 "backup_info": backup_info,
                    //             });

                    //             client.publish(
                    //                 "warden",  // Using default exchange
                    //                 routing_key,
                    //                 MessageType::Notification,
                    //                 &serde_json::to_string(&response_payload)?,
                    //             ).await?;
                    //         },
                    //         Err(e) => {
                    //             error!("Failed to create emergency backup: {}", e);

                    //             // Notify about the failure
                    //             let routing_key = "warden.notifications.postgres";
                    //             let response_payload = serde_json::json!({
                    //                 "action": "emergency_backup",
                    //                 "status": "failed",
                    //                 "database": db_name,
                    //                 "error": e.to_string(),
                    //             });

                    //             client.publish(
                    //                 "warden",  // Using default exchange
                    //                 routing_key,
                    //                 MessageType::Notification,
                    //                 &serde_json::to_string(&response_payload)?,
                    //             ).await?;
                    //         }
                    //     }
                    // }
                }
                info!("PostgreSQL event: {} - {}", event.severity, event.message);
            }
        }
        _ => {
            // For non-critical events, we just log them
            info!("PostgreSQL event: {} - {}", event.severity, event.message);
        }
    }

    Ok(())
}

/// Handle Overwatch events
async fn handle_overwatch_event(event: &EventPayload, client: &Arc<AmqpClient>) -> Result<()> {
    match event.severity {
        EventSeverity::Error | EventSeverity::Critical => {
            // For error or critical Overwatch events, we might want to restart the service
            info!(
                "Attempting to restart Overwatch due to {} event",
                event.severity
            );

            // Import the overwatch crate
            use overwatch::control;

            // Stop Overwatch
            match control::stop().await {
                Ok(_) => {
                    info!("Overwatch stopped successfully");

                    // Wait a moment before starting again
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                    // Start Overwatch
                    match control::start().await {
                        Ok(_) => {
                            info!("Overwatch restarted successfully");

                            // Notify about the restart
                            let routing_key = "warden.notifications.overwatch";
                            let response_payload = serde_json::json!({
                                "action": "restart",
                                "status": "success",
                                "reason": event.message,
                            });

                            client
                                .publish(
                                    "warden", // Using default exchange
                                    routing_key,
                                    MessageType::Notification,
                                    &serde_json::to_string(&response_payload)?,
                                )
                                .await?;
                        }
                        Err(e) => {
                            error!("Failed to start Overwatch: {}", e);

                            // Notify about the failure
                            let routing_key = "warden.notifications.overwatch";
                            let response_payload = serde_json::json!({
                                "action": "restart",
                                "status": "failed",
                                "reason": event.message,
                                "error": e.to_string(),
                            });

                            client
                                .publish(
                                    "warden", // Using default exchange
                                    routing_key,
                                    MessageType::Notification,
                                    &serde_json::to_string(&response_payload)?,
                                )
                                .await?;
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to stop Overwatch: {}", e);

                    // Notify about the failure
                    let routing_key = "warden.notifications.overwatch";
                    let response_payload = serde_json::json!({
                        "action": "restart",
                        "status": "failed",
                        "reason": event.message,
                        "error": e.to_string(),
                    });

                    client
                        .publish(
                            "warden", // Using default exchange
                            routing_key,
                            MessageType::Notification,
                            &serde_json::to_string(&response_payload)?,
                        )
                        .await?;
                }
            }
        }
        _ => {
            // For non-critical events, we just log them
            info!("Overwatch event: {} - {}", event.severity, event.message);
        }
    }

    Ok(())
}

/// Handle system events
async fn handle_system_event(event: &EventPayload, _client: &Arc<AmqpClient>) -> Result<()> {
    // Log all system events
    info!("System event: {} - {}", event.severity, event.message);

    // For now, we just acknowledge system events without taking specific actions
    // This could be expanded in the future to handle different types of system events

    Ok(())
}
