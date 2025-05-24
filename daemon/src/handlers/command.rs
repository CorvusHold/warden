use crate::amqp::{AmqpClient, MessageType};
use anyhow::{anyhow, Result};
use common::config::WardenConfig;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Helper function to extract subtopic from routing key
fn get_subtopic(routing_key: &str, prefix: &str) -> Option<String> {
    routing_key
        .strip_prefix(prefix)
        .map(|stripped| stripped.to_string())
}

/// Command types that the daemon can handle
#[derive(Debug, Serialize, Deserialize)]
pub enum CommandType {
    Status,
    Restart,
    ConfigGet,
    ConfigSet,
    PostgresBackup,
    PostgresRestore,
    OverwatchStatus,
    OverwatchStart,
    OverwatchStop,
    Custom(String),
}

/// Command payload structure
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandPayload {
    pub command_type: CommandType,
    pub args: Option<HashMap<String, serde_json::Value>>,
}

/// Response payload structure
#[derive(Debug, Serialize, Deserialize)]
pub struct ResponsePayload {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

/// Handle command messages
pub async fn handle_command(
    routing_key: &str,
    payload: &str,
    client: &Arc<AmqpClient>,
    config: &Arc<Mutex<WardenConfig>>,
) -> Result<()> {
    // Extract command subtopic from routing key
    let subtopic = get_subtopic(routing_key, "warden.commands.")
        .ok_or_else(|| anyhow!("Invalid command routing key format"))?;

    // Parse command payload
    let command = match serde_json::from_str::<CommandPayload>(payload) {
        Ok(cmd) => cmd,
        Err(e) => {
            let error_msg = format!("Failed to parse command payload: {}", e);
            error!("{}", error_msg);

            // Send error response
            let response = ResponsePayload {
                success: false,
                message: error_msg.clone(),
                data: None,
            };

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

            client
                .publish(
                    &exchange,
                    &format!("warden.responses.{}", subtopic),
                    MessageType::Response,
                    &serde_json::to_string(&response)?,
                )
                .await?;

            return Err(anyhow!(error_msg.clone()));
        }
    };

    info!("Received command: {:?}", command);

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

    // Process command based on type
    let response = match command.command_type {
        CommandType::Status => {
            // Return daemon status
            ResponsePayload {
                success: true,
                message: "Daemon is running".to_string(),
                data: Some(serde_json::json!({
                    "status": "running",
                    "features": {
                        "overwatch": config.lock().unwrap().features.overwatch,
                        "postgres_backup": config.lock().unwrap().features.postgres_backup,
                    }
                })),
            }
        }
        CommandType::ConfigGet => {
            // Return current configuration
            let config_guard = config.lock().unwrap();
            ResponsePayload {
                success: true,
                message: "Current configuration".to_string(),
                data: Some(serde_json::to_value(&*config_guard).unwrap_or_default()),
            }
        }
        CommandType::ConfigSet => {
            // Update configuration with provided values
            if let Some(args) = &command.args {
                if let Some(config_json) = args.get("config") {
                    match serde_json::from_value::<WardenConfig>(config_json.clone()) {
                        Ok(new_config) => {
                            // Update the config
                            {
                                let mut config_guard = config.lock().unwrap();
                                *config_guard = new_config;
                            }

                            // Save the config to disk
                            let config_guard = config.lock().unwrap();
                            if let Err(e) = common::config::update_config(&config_guard) {
                                error!("Failed to save config: {}", e);
                                ResponsePayload {
                                    success: false,
                                    message: format!("Failed to save config: {}", e),
                                    data: None,
                                }
                            } else {
                                ResponsePayload {
                                    success: true,
                                    message: "Configuration updated".to_string(),
                                    data: None,
                                }
                            }
                        }
                        Err(e) => ResponsePayload {
                            success: false,
                            message: format!("Invalid configuration format: {}", e),
                            data: None,
                        },
                    }
                } else {
                    ResponsePayload {
                        success: false,
                        message: "Missing 'config' parameter".to_string(),
                        data: None,
                    }
                }
            } else {
                ResponsePayload {
                    success: false,
                    message: "No arguments provided".to_string(),
                    data: None,
                }
            }
        }
        CommandType::PostgresBackup => {
            // Check if PostgreSQL backup feature is enabled
            if !config.lock().unwrap().features.postgres_backup {
                ResponsePayload {
                    success: false,
                    message: "PostgreSQL backup feature is not enabled".to_string(),
                    data: None,
                }
            } else {
                // Call PostgreSQL backup functionality
                // match postgres::backup::full::create_backup().await {
                //     Ok(backup_info) => {
                //         ResponsePayload {
                //             success: true,
                //             message: "Backup created successfully".to_string(),
                //             data: Some(serde_json::to_value(backup_info).unwrap_or_default()),
                //         }
                //     },
                //     Err(e) => {
                //         ResponsePayload {
                //             success: false,
                //             message: format!("Failed to create backup: {}", e),
                //             data: None,
                //         }
                //     }
                // }
                ResponsePayload {
                    success: false,
                    message: "PostgreSQL backup feature is not implemented yet".to_string(),
                    data: None,
                }
            }
        }
        CommandType::PostgresRestore => {
            // Check if PostgreSQL backup feature is enabled
            if !config.lock().unwrap().features.postgres_backup {
                ResponsePayload {
                    success: false,
                    message: "PostgreSQL backup feature is not enabled".to_string(),
                    data: None,
                }
            } else if let Some(args) = &command.args {
                if let Some(backup_id) = args.get("backup_id") {
                    if let Some(_backup_id) = backup_id.as_str() {
                        // Call PostgreSQL restore functionality
                        // match postgres::backup::restore_backup(backup_id).await {
                        //     Ok(_) => {
                        //         ResponsePayload {
                        //             success: true,
                        //             message: "Backup restored successfully".to_string(),
                        //             data: None,
                        //         }
                        //     },
                        //     Err(e) => {
                        //         ResponsePayload {
                        //             success: false,
                        //             message: format!("Failed to restore backup: {}", e),
                        //             data: None,
                        //         }
                        //     }
                        // }
                        ResponsePayload {
                            success: false,
                            message: "PostgreSQL restore feature is not implemented yet"
                                .to_string(),
                            data: None,
                        }
                    } else {
                        ResponsePayload {
                            success: false,
                            message: "Invalid backup_id format".to_string(),
                            data: None,
                        }
                    }
                } else {
                    ResponsePayload {
                        success: false,
                        message: "Missing 'backup_id' parameter".to_string(),
                        data: None,
                    }
                }
            } else {
                ResponsePayload {
                    success: false,
                    message: "No arguments provided".to_string(),
                    data: None,
                }
            }
        }
        CommandType::OverwatchStatus => {
            // Check if Overwatch feature is enabled
            if !config.lock().unwrap().features.overwatch {
                ResponsePayload {
                    success: false,
                    message: "Overwatch feature is not enabled".to_string(),
                    data: None,
                }
            } else {
                // Get Overwatch status
                // match overwatch::status::get_status().await {
                //     Ok(status) => {
                //         ResponsePayload {
                //             success: true,
                //             message: "Overwatch status".to_string(),
                //             data: Some(serde_json::to_value(status).unwrap_or_default()),
                //         }
                //     },
                //     Err(e) => {
                //         ResponsePayload {
                //             success: false,
                //             message: format!("Failed to get Overwatch status: {}", e),
                //             data: None,
                //         }
                //     }
                // }
                ResponsePayload {
                    success: false,
                    message: "Overwatch feature is not implemented yet".to_string(),
                    data: None,
                }
            }
        }
        CommandType::OverwatchStart => {
            // Check if Overwatch feature is enabled
            if !config.lock().unwrap().features.overwatch {
                ResponsePayload {
                    success: false,
                    message: "Overwatch feature is not enabled".to_string(),
                    data: None,
                }
            } else {
                // Start Overwatch
                match overwatch::control::start().await {
                    Ok(_) => ResponsePayload {
                        success: true,
                        message: "Overwatch started".to_string(),
                        data: None,
                    },
                    Err(e) => ResponsePayload {
                        success: false,
                        message: format!("Failed to start Overwatch: {}", e),
                        data: None,
                    },
                }
            }
        }
        CommandType::OverwatchStop => {
            // Check if Overwatch feature is enabled
            if !config.lock().unwrap().features.overwatch {
                ResponsePayload {
                    success: false,
                    message: "Overwatch feature is not enabled".to_string(),
                    data: None,
                }
            } else {
                // Stop Overwatch
                match overwatch::control::stop().await {
                    Ok(_) => ResponsePayload {
                        success: true,
                        message: "Overwatch stopped".to_string(),
                        data: None,
                    },
                    Err(e) => ResponsePayload {
                        success: false,
                        message: format!("Failed to stop Overwatch: {}", e),
                        data: None,
                    },
                }
            }
        }
        CommandType::Restart => {
            // Signal that the daemon should restart
            // This will be handled by the main process
            ResponsePayload {
                success: true,
                message: "Daemon restart requested".to_string(),
                data: None,
            }
        }
        CommandType::Custom(cmd_name) => {
            // Handle custom commands
            warn!("Received custom command: {}", cmd_name);
            ResponsePayload {
                success: false,
                message: format!("Custom command '{}' not implemented", cmd_name),
                data: None,
            }
        }
    };

    // Send response
    client
        .publish(
            &exchange,
            &format!("warden.responses.{}", subtopic),
            MessageType::Response,
            &serde_json::to_string(&response)?,
        )
        .await?;

    Ok(())
}
