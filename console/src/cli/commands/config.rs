use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use common::config::{load_config, update_config};
use log::info;
use serde_json;
use serde_yaml;

#[derive(Debug, Args)]
pub struct Config {
    #[clap(subcommand)]
    command: ConfigCommands,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    /// Get the current configuration
    Get {
        /// Output format (json, yaml, toml, text)
        #[clap(long, default_value = "text")]
        format: String,
    },
    /// Set a configuration value
    Set {
        /// Configuration key (e.g., c2_server, c2_auth.id, features.Overwatch)
        key: String,
        /// Value to set
        value: String,
    },
}

impl Config {
    pub async fn run(self) -> Result<()> {
        match self.command {
            ConfigCommands::Get { format } => {
                let config = load_config()?;

                match format.as_str() {
                    "json" => {
                        let json = serde_json::to_string_pretty(&config)?;
                        info!("{}", json);
                    }
                    "yaml" => {
                        let yaml = serde_yaml::to_string(&config)?;
                        info!("{}", yaml);
                    }
                    "toml" => {
                        let toml = toml::to_string_pretty(&config)?;
                        info!("{}", toml);
                    }
                    "text" => {
                        info!("Warden Configuration:");
                        info!("  C2 Server: {}", config.c2_server);
                        info!("  C2 Auth:");
                        info!(
                            "    ID: {}",
                            if config.c2_auth.id.is_empty() {
                                "<not set>"
                            } else {
                                &config.c2_auth.id
                            },
                        );
                        info!(
                            "    Secret: {}",
                            if config.c2_auth.secret.is_empty() {
                                "<not set>"
                            } else {
                                "<set>"
                            },
                        );
                        info!("  Features:");
                        info!("    Overwatch: {}", config.features.overwatch);
                        info!("    PostgresBackup: {}", config.features.postgres_backup);
                    }
                    _ => {
                        return Err(anyhow!("Invalid format: {}", format));
                    }
                }
            }
            ConfigCommands::Set { key, value } => {
                // Load the current configuration
                let mut config = load_config()?;

                // Update the configuration based on the key
                match key.as_str() {
                    "c2_server" => {
                        config.c2_server = value;
                    }
                    "c2_auth.id" => {
                        config.c2_auth.id = value;
                    }
                    "c2_auth.secret" => {
                        config.c2_auth.secret = value;
                    }
                    "features.Overwatch" => {
                        config.features.overwatch = value.parse::<bool>().map_err(|_| {
                            anyhow!("Invalid boolean value for features.Overwatch: {}", value)
                        })?;
                    }
                    "features.PostgresBackup" => {
                        config.features.postgres_backup = value.parse::<bool>().map_err(|_| {
                            anyhow!(
                                "Invalid boolean value for features.PostgresBackup: {}",
                                value
                            )
                        })?;
                    }
                    _ => {
                        return Err(anyhow!("Unknown configuration key: {}", key));
                    }
                }

                // Save the updated configuration
                update_config(&config)
                    .map_err(|e| anyhow!("Failed to update configuration: {}", e))?;
                info!("Configuration updated successfully.");
            }
        }

        Ok(())
    }
}
