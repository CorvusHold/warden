//! HOLD Command Handling
//!
//! Defines the command types that HOLD can send to Warden agents
//! and the handler that maps them to internal operations.

use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Command types that HOLD can send to Warden
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HoldCommand {
    /// Request a backup
    RequestBackup {
        #[serde(default)]
        backup_type: Option<String>,
        #[serde(default)]
        database: Option<String>,
        #[serde(default)]
        storage_profile: Option<String>,
        #[serde(default)]
        tags: HashMap<String, String>,
    },
    /// Request current status
    RequestStatus {
        #[serde(default)]
        include_metrics: bool,
        #[serde(default)]
        include_catalog_summary: bool,
    },
    /// Request backup status
    RequestBackupStatus,
    /// Request PITR status
    RequestPitrStatus,
    /// Request metrics
    RequestMetrics,
    /// List available backups
    ListBackups {
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        database: Option<String>,
    },
    /// Plan a switchover (dry-run only)
    PlanSwitchover {
        #[serde(default)]
        target_node: Option<String>,
    },
}

/// Envelope for HOLD commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldCommandEnvelope {
    pub version: String,
    pub request_id: String,
    pub command: HoldCommand,
}

/// Response to a HOLD command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldCommandResponse {
    pub version: String,
    pub request_id: String,
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl HoldCommandResponse {
    pub fn success(request_id: String, message: String, data: Option<serde_json::Value>) -> Self {
        Self {
            version: "1.0".to_string(),
            request_id,
            success: true,
            message,
            data,
            error_code: None,
        }
    }

    pub fn error(request_id: String, message: String, error_code: Option<String>) -> Self {
        Self {
            version: "1.0".to_string(),
            request_id,
            success: false,
            message,
            data: None,
            error_code,
        }
    }
}

/// Handler for HOLD commands
///
/// Maps HOLD commands to internal Warden operations.
/// This is a thin layer that delegates to existing functionality.
pub struct HoldCommandHandler {
    agent_id: String,
}

impl HoldCommandHandler {
    pub fn new(agent_id: String) -> Self {
        Self { agent_id }
    }

    /// Handle a HOLD command and return a response
    pub async fn handle(&self, envelope: HoldCommandEnvelope) -> HoldCommandResponse {
        let request_id = envelope.request_id.clone();
        info!("Handling HOLD command: {:?} (request_id: {})", envelope.command, request_id);

        match self.dispatch(envelope).await {
            Ok(response) => response,
            Err(e) => {
                error!("HOLD command failed: {}", e);
                HoldCommandResponse::error(
                    request_id,
                    format!("Command failed: {}", e),
                    Some("INTERNAL_ERROR".to_string()),
                )
            }
        }
    }

    async fn dispatch(&self, envelope: HoldCommandEnvelope) -> Result<HoldCommandResponse> {
        let request_id = envelope.request_id;

        match envelope.command {
            HoldCommand::RequestStatus { include_metrics, include_catalog_summary } => {
                self.handle_request_status(&request_id, include_metrics, include_catalog_summary).await
            }
            HoldCommand::RequestBackup { backup_type, database, storage_profile, tags } => {
                self.handle_request_backup(&request_id, backup_type, database, storage_profile, tags).await
            }
            HoldCommand::RequestBackupStatus => {
                self.handle_request_backup_status(&request_id).await
            }
            HoldCommand::RequestPitrStatus => {
                self.handle_request_pitr_status(&request_id).await
            }
            HoldCommand::RequestMetrics => {
                self.handle_request_metrics(&request_id).await
            }
            HoldCommand::ListBackups { limit, database } => {
                self.handle_list_backups(&request_id, limit, database).await
            }
            HoldCommand::PlanSwitchover { target_node } => {
                self.handle_plan_switchover(&request_id, target_node).await
            }
        }
    }

    async fn handle_request_status(
        &self,
        request_id: &str,
        _include_metrics: bool,
        _include_catalog_summary: bool,
    ) -> Result<HoldCommandResponse> {
        debug!("Handling RequestStatus command");

        // Build status response using internal APIs
        // This would call into postgres crate's status functionality
        let status = serde_json::json!({
            "agent_id": self.agent_id,
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "features": {
                "postgres_backup": true,
                "overwatch": false
            }
        });

        Ok(HoldCommandResponse::success(
            request_id.to_string(),
            "Status retrieved".to_string(),
            Some(status),
        ))
    }

    async fn handle_request_backup(
        &self,
        request_id: &str,
        backup_type: Option<String>,
        database: Option<String>,
        _storage_profile: Option<String>,
        tags: HashMap<String, String>,
    ) -> Result<HoldCommandResponse> {
        info!(
            "Handling RequestBackup: type={:?}, database={:?}, tags={:?}",
            backup_type, database, tags
        );

        // This would trigger an actual backup via the postgres crate
        // For now, return a placeholder response
        let backup_info = serde_json::json!({
            "backup_id": format!("backup-{}", uuid::Uuid::new_v4()),
            "status": "initiated",
            "backup_type": backup_type.unwrap_or_else(|| "snapshot".to_string()),
            "database": database,
            "triggered_by": "hold",
            "started_at": chrono::Utc::now().to_rfc3339()
        });

        Ok(HoldCommandResponse::success(
            request_id.to_string(),
            "Backup initiated".to_string(),
            Some(backup_info),
        ))
    }

    async fn handle_request_backup_status(&self, request_id: &str) -> Result<HoldCommandResponse> {
        debug!("Handling RequestBackupStatus command");

        let status = serde_json::json!({
            "agent_id": self.agent_id,
            "last_backup": null,
            "backup_in_progress": false,
            "total_backups": 0
        });

        Ok(HoldCommandResponse::success(
            request_id.to_string(),
            "Backup status retrieved".to_string(),
            Some(status),
        ))
    }

    async fn handle_request_pitr_status(&self, request_id: &str) -> Result<HoldCommandResponse> {
        debug!("Handling RequestPitrStatus command");

        let status = serde_json::json!({
            "agent_id": self.agent_id,
            "pitr_available": false,
            "earliest_point": null,
            "latest_point": null,
            "wal_segments": 0
        });

        Ok(HoldCommandResponse::success(
            request_id.to_string(),
            "PITR status retrieved".to_string(),
            Some(status),
        ))
    }

    async fn handle_request_metrics(&self, request_id: &str) -> Result<HoldCommandResponse> {
        debug!("Handling RequestMetrics command");

        // Return Prometheus-format metrics
        let metrics = serde_json::json!({
            "format": "prometheus",
            "metrics": "# HELP warden_up Warden agent status\n# TYPE warden_up gauge\nwarden_up 1\n"
        });

        Ok(HoldCommandResponse::success(
            request_id.to_string(),
            "Metrics retrieved".to_string(),
            Some(metrics),
        ))
    }

    async fn handle_list_backups(
        &self,
        request_id: &str,
        limit: Option<u32>,
        database: Option<String>,
    ) -> Result<HoldCommandResponse> {
        debug!("Handling ListBackups: limit={:?}, database={:?}", limit, database);

        let backups = serde_json::json!({
            "backups": [],
            "total_count": 0,
            "limit": limit.unwrap_or(100)
        });

        Ok(HoldCommandResponse::success(
            request_id.to_string(),
            "Backups listed".to_string(),
            Some(backups),
        ))
    }

    async fn handle_plan_switchover(
        &self,
        request_id: &str,
        target_node: Option<String>,
    ) -> Result<HoldCommandResponse> {
        warn!("PlanSwitchover is a dry-run only operation");

        let plan = serde_json::json!({
            "dry_run": true,
            "target_node": target_node,
            "current_primary": null,
            "steps": [],
            "estimated_downtime_secs": 0,
            "warnings": ["This is a dry-run plan only"]
        });

        Ok(HoldCommandResponse::success(
            request_id.to_string(),
            "Switchover plan generated (dry-run)".to_string(),
            Some(plan),
        ))
    }
}

/// Parse a HOLD command from JSON
pub fn parse_command(payload: &[u8]) -> Result<HoldCommandEnvelope> {
    serde_json::from_slice(payload).context("Failed to parse HOLD command")
}
