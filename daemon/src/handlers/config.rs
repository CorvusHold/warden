use anyhow::{Context, Result};
use common::config::WardenConfig;
use log::{error, info};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// Handle configuration update messages
pub async fn handle_config_update(payload: &str, config: &Arc<Mutex<WardenConfig>>) -> Result<()> {
    // Parse the configuration update
    let config_update: Value =
        serde_json::from_str(payload).context("Failed to parse configuration update payload")?;

    // Merge the update with the current configuration
    let mut current_config = config.lock().unwrap();

    // Update C2 server if provided
    if let Some(c2_server) = config_update.get("c2_server").and_then(|v| v.as_str()) {
        info!("Updating C2 server to: {}", c2_server);
        current_config.c2_server = c2_server.to_string();
    }

    // Update C2 auth if provided
    if let Some(c2_auth) = config_update.get("c2_auth") {
        if let Some(id) = c2_auth.get("id").and_then(|v| v.as_str()) {
            info!("Updating C2 auth ID");
            current_config.c2_auth.id = id.to_string();
        }

        if let Some(secret) = c2_auth.get("secret").and_then(|v| v.as_str()) {
            info!("Updating C2 auth secret");
            current_config.c2_auth.secret = secret.to_string();
        }
    }

    // Update features if provided
    if let Some(features) = config_update.get("features") {
        if let Some(overwatch) = features.get("Overwatch").and_then(|v| v.as_bool()) {
            info!("Updating Overwatch feature to: {}", overwatch);
            current_config.features.overwatch = overwatch;
        }

        if let Some(postgres_backup) = features.get("PostgresBackup").and_then(|v| v.as_bool()) {
            info!("Updating PostgreSQL backup feature to: {}", postgres_backup);
            current_config.features.postgres_backup = postgres_backup;
        }
    }

    // Save the updated configuration to disk
    match common::config::update_config(&current_config) {
        Ok(_) => {
            info!("Configuration updated and saved to disk");
            Ok(())
        }
        Err(e) => {
            error!("Failed to save updated configuration: {}", e);
            Err(anyhow::anyhow!(
                "Failed to save updated configuration: {}",
                e
            ))
        }
    }
}
