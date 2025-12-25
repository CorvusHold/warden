//! HOLD Event Publishing
//!
//! Defines events that Warden publishes to HOLD and the publisher
//! that handles periodic and on-demand event publishing.

use chrono::{DateTime, Utc};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::client::HoldClient;

/// Event types that Warden publishes to HOLD
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HoldEvent {
    /// Periodic heartbeat
    Heartbeat(HeartbeatPayload),
    /// Status update (on request or periodic)
    Status(StatusPayload),
    /// Metrics update
    Metrics(MetricsPayload),
    /// Backup completed
    BackupCompleted(BackupCompletedPayload),
    /// Backup failed
    BackupFailed(BackupFailedPayload),
    /// Catalog updated
    CatalogUpdated(CatalogUpdatedPayload),
}

/// Heartbeat payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatPayload {
    pub status: String,
    pub uptime_secs: u64,
    pub features: HashMap<String, bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_backup_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitr_window_hours: Option<u32>,
}

/// Status payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPayload {
    pub health: String,
    pub backup: BackupStatusSummary,
    pub pitr: PitrStatusSummary,
    pub storage: StorageStatusSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStatusSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_successful: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_type: Option<String>,
    pub total_count: u32,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitrStatusSummary {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest_point: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_point: Option<DateTime<Utc>>,
    pub wal_segments: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStatusSummary {
    pub local_used_bytes: u64,
    pub remote_used_bytes: u64,
}

/// Metrics payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsPayload {
    pub format: String,
    pub metrics: String,
}

/// Backup completed payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupCompletedPayload {
    pub backup_id: String,
    pub backup_type: String,
    pub database: Option<String>,
    pub size_bytes: u64,
    pub duration_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_location: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

/// Backup failed payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFailedPayload {
    pub backup_id: String,
    pub backup_type: String,
    pub database: Option<String>,
    pub error_code: String,
    pub error_message: String,
    pub started_at: DateTime<Utc>,
    pub failed_at: DateTime<Utc>,
}

/// Catalog updated payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogUpdatedPayload {
    pub total_backups: u32,
    pub total_size_bytes: u64,
    pub oldest_backup: Option<DateTime<Utc>>,
    pub newest_backup: Option<DateTime<Utc>>,
}

/// Event envelope for publishing to HOLD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldEventEnvelope {
    pub version: String,
    pub event: EventMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation: Option<CorrelationInfo>,
    pub payload: HoldEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMeta {
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationInfo {
    pub request_id: String,
}

impl HoldEventEnvelope {
    pub fn new(agent_id: String, event: HoldEvent) -> Self {
        let event_type = match &event {
            HoldEvent::Heartbeat(_) => "hold.heartbeat",
            HoldEvent::Status(_) => "hold.status",
            HoldEvent::Metrics(_) => "hold.metrics",
            HoldEvent::BackupCompleted(_) => "hold.backup.completed",
            HoldEvent::BackupFailed(_) => "hold.backup.failed",
            HoldEvent::CatalogUpdated(_) => "hold.catalog.updated",
        };

        Self {
            version: "1.0".to_string(),
            event: EventMeta {
                event_type: event_type.to_string(),
                timestamp: Utc::now(),
                agent_id,
            },
            correlation: None,
            payload: event,
        }
    }

    pub fn with_correlation(mut self, request_id: String) -> Self {
        self.correlation = Some(CorrelationInfo { request_id });
        self
    }

    pub fn routing_key(&self) -> String {
        format!("warden.events.{}", self.event.event_type)
    }
}

/// Publisher for HOLD events
pub struct HoldEventPublisher {
    client: Arc<HoldClient>,
    agent_id: String,
    start_time: std::time::Instant,
}

impl HoldEventPublisher {
    pub fn new(client: Arc<HoldClient>, agent_id: String) -> Self {
        Self {
            client,
            agent_id,
            start_time: std::time::Instant::now(),
        }
    }

    /// Publish a heartbeat event
    pub async fn publish_heartbeat(&self) {
        let payload = HeartbeatPayload {
            status: "healthy".to_string(),
            uptime_secs: self.start_time.elapsed().as_secs(),
            features: [
                ("postgres_backup".to_string(), true),
                ("overwatch".to_string(), false),
            ]
            .into_iter()
            .collect(),
            last_backup_at: None,
            pitr_window_hours: None,
        };

        let envelope = HoldEventEnvelope::new(self.agent_id.clone(), HoldEvent::Heartbeat(payload));

        self.publish_event(envelope).await;
    }

    /// Publish a status event
    pub async fn publish_status(&self, correlation_id: Option<String>) {
        let payload = StatusPayload {
            health: "healthy".to_string(),
            backup: BackupStatusSummary {
                last_successful: None,
                last_type: None,
                total_count: 0,
                total_size_bytes: 0,
            },
            pitr: PitrStatusSummary {
                available: false,
                earliest_point: None,
                latest_point: None,
                wal_segments: 0,
            },
            storage: StorageStatusSummary {
                local_used_bytes: 0,
                remote_used_bytes: 0,
            },
        };

        let mut envelope =
            HoldEventEnvelope::new(self.agent_id.clone(), HoldEvent::Status(payload));

        if let Some(id) = correlation_id {
            envelope = envelope.with_correlation(id);
        }

        self.publish_event(envelope).await;
    }

    /// Publish a backup completed event
    pub async fn publish_backup_completed(&self, payload: BackupCompletedPayload) {
        let envelope =
            HoldEventEnvelope::new(self.agent_id.clone(), HoldEvent::BackupCompleted(payload));
        self.publish_event(envelope).await;
    }

    /// Publish a backup failed event
    pub async fn publish_backup_failed(&self, payload: BackupFailedPayload) {
        let envelope =
            HoldEventEnvelope::new(self.agent_id.clone(), HoldEvent::BackupFailed(payload));
        self.publish_event(envelope).await;
    }

    /// Publish an event to HOLD
    async fn publish_event(&self, envelope: HoldEventEnvelope) {
        let routing_key = envelope.routing_key();

        match serde_json::to_vec(&envelope) {
            Ok(payload) => match self.client.publish(&routing_key, &payload).await {
                Ok(Some(_)) => {
                    debug!("Published event to HOLD: {}", routing_key);
                }
                Ok(None) => {
                    debug!("HOLD not connected, event not published: {}", routing_key);
                }
                Err(e) => {
                    warn!("Failed to publish event to HOLD: {}", e);
                }
            },
            Err(e) => {
                warn!("Failed to serialize event for HOLD: {}", e);
            }
        }
    }
}
