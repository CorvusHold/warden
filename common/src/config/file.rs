use config::{Config, ConfigError, File};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use log::{error, info};

#[derive(Debug, Deserialize, Serialize)]
pub struct WardenConfig {
    pub c2_server: String,
    pub c2_auth: C2AuthConfig,
    pub features: FeaturesConfig,
    pub mqtt: Option<MqttConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MqttConfig {
    pub broker: String,
    pub port: Option<u16>,
    pub client_id: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub topics: Option<Vec<String>>,
    // AMQP specific fields
    pub vhost: Option<String>,
    pub exchange: Option<String>,
    pub queues: Option<Vec<String>>,
    pub protocol: Option<String>, // "mqtt" or "amqp"
}

#[derive(Debug, Deserialize, Serialize)]
pub struct C2AuthConfig {
    pub id: String,
    pub secret: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FeaturesConfig {
    #[serde(rename = "Overwatch")]
    pub overwatch: bool,
    #[serde(rename = "PostgresBackup")]
    pub postgres_backup: bool,
}

pub fn load_config() -> Result<WardenConfig, ConfigError> {
    let config_paths = [
        "/etc/warden/warden.toml",
        "~/.config/warden/warden.toml",
        "warden.toml",
    ];

    // Create config builder and apply default values
    let config_builder = Config::builder()
        .set_default("c2_server", "http://localhost:8080")?
        .set_default("c2_auth.id", "")?
        .set_default("c2_auth.secret", "")?
        .set_default("features.Overwatch", false)?
        .set_default("features.PostgresBackup", false)?
        .set_default("mqtt.broker", "localhost")?
        .set_default("mqtt.port", 1883)?
        .set_default("mqtt.client_id", None::<String>)?
        .set_default("mqtt.username", None::<String>)?
        .set_default("mqtt.password", None::<String>)?
        .set_default(
            "mqtt.topics",
            vec!["warden/commands/#", "warden/config", "warden/events/#"],
        )?;

    // Add config sources
    let config_builder = config_paths.iter().fold(config_builder, |builder, path| {
        let path = shellexpand::full(path).unwrap().into_owned();
        if Path::new(&path).exists() {
            builder.add_source(File::with_name(&path))
        } else {
            builder
        }
    });

    // Build and deserialize
    config_builder.build()?.try_deserialize()
}

/// Updates the configuration file with the provided config values
///
/// This function will write to the first available config file path in the following order:
/// 1. warden.toml (current directory)
/// 2. ~/.config/warden/warden.toml
/// 3. /etc/warden/warden.toml (if writable)
pub fn update_config(config: &WardenConfig) -> Result<(), Box<dyn std::error::Error>> {
    let config_paths = [
        "/etc/warden/warden.toml",
        "~/.config/warden/warden.toml",
        "warden.toml",
    ];

    // Convert the config to TOML format
    let toml_string = toml::to_string_pretty(config)?;

    // Try to write to the first available path
    for path in config_paths {
        let expanded_path = shellexpand::full(path).unwrap().into_owned();
        let path_obj = Path::new(&expanded_path);

        // If the directory doesn't exist, try to create it
        if let Some(parent) = path_obj.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    // If we can't create the directory, try the next path
                    error!("Failed to create directory {}: {}", parent.display(), e);
                    continue;
                }
            }
        }

        // Try to write to the file
        match fs::File::create(path_obj) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(toml_string.as_bytes()) {
                    error!("Failed to write to {}: {}", expanded_path, e);
                    continue;
                }

                info!("Configuration updated successfully at {}", expanded_path);
                return Ok(());
            }
            Err(e) => {
                error!("Failed to create file {}: {}", expanded_path, e);
                continue;
            }
        }
    }

    Err("Failed to update configuration: could not write to any config file path".into())
}
