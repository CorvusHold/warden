// options
// format: string (json, yaml, text, color (default))

// commands
// status [OPTIONS]

// Render
// Warden version: <current version>
// Hold version: <hold version>
// Compatible: <yes/no>
// Last sync: <last sync time>
//
// Warden services:
// Overwatch: <running/stopped>
// Postgres backup: <running/stopped>

use anyhow::{anyhow, Result};
use clap::Args;
use common::config::load_config;
use log::info;
use serde::Serialize;
use serde_json;
use serde_yaml;

#[derive(Debug, Args)]
pub struct Status {
    /// Output format (json, yaml, text, color)
    #[clap(long, default_value = "color")]
    format: String,
}

#[derive(Serialize)]
struct WardenStatus {
    warden_version: String,
    hold_version: String,
    compatible: bool,
    last_sync: String,
    overwatch: String,
    postgres_backup: String,
}

impl Status {
    pub async fn run(self) -> Result<()> {
        info!("Getting status...");

        // Load the configuration to get the feature status
        let config = load_config()?;

        // TODO: In the future, implement actual API calls to get real-time status
        // For now, we'll use the configuration to determine the status

        let status = WardenStatus {
            warden_version: env!("CARGO_PKG_VERSION").to_string(),
            hold_version: "0.2.0".to_string(), // This would come from an API call in the future
            compatible: true,                  // This would come from an API call in the future
            last_sync: "2024-01-01T00:00:00Z".to_string(), // This would come from an API call
            overwatch: if config.features.overwatch {
                "running"
            } else {
                "stopped"
            }
            .to_string(),
            postgres_backup: if config.features.postgres_backup {
                "running"
            } else {
                "stopped"
            }
            .to_string(),
        };

        match self.format.as_str() {
            "json" => {
                let json = serde_json::to_string_pretty(&status)?;
                info!("{json}");
            }
            "yaml" => {
                let yaml = serde_yaml::to_string(&status)?;
                info!("{yaml}");
            }
            "text" | "color" => {
                info!("Warden version: {}", status.warden_version);
                info!("Hold version: {}", status.hold_version);
                info!("Compatible: {}", status.compatible);
                info!("Daemon last sync: {}", status.last_sync);

                info!("\nWarden services:");
                info!("  Overwatch: {}", status.overwatch);
                info!("  Postgres backup: {}", status.postgres_backup);
            }
            _ => {
                return Err(anyhow!("Invalid format: {}", self.format));
            }
        }

        Ok(())
    }
}
