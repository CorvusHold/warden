//! Standard event types for Warden operations.

use chrono::{DateTime, Utc};
use serde::de::Error as DeError;
use serde::{Deserialize, Serialize};
use serde::{Deserializer, Serializer};
use std::collections::HashMap;

/// Event severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventSeverity {
    /// Informational event
    Info,
    /// Warning event
    Warning,
    /// Critical/error event
    Critical,
}

impl std::fmt::Display for EventSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventSeverity::Info => write!(f, "info"),
            EventSeverity::Warning => write!(f, "warning"),
            EventSeverity::Critical => write!(f, "critical"),
        }
    }
}

/// Event categories for grouping related events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventCategory {
    /// Backup-related events
    Backup,
    /// Restore-related events
    Restore,
    /// PITR-related events
    Pitr,
    /// Retention/purge-related events
    Retention,
    /// HA orchestration events
    Ha,
    /// Status/health events
    Status,
}

impl std::fmt::Display for EventCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventCategory::Backup => write!(f, "backup"),
            EventCategory::Restore => write!(f, "restore"),
            EventCategory::Pitr => write!(f, "pitr"),
            EventCategory::Retention => write!(f, "retention"),
            EventCategory::Ha => write!(f, "ha"),
            EventCategory::Status => write!(f, "status"),
        }
    }
}

/// Standard event types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType {
    // Backup events
    BackupStarted,
    BackupCompleted,
    BackupFailed,

    // Restore events
    RestoreStarted,
    RestoreCompleted,
    RestoreFailed,

    // PITR events
    PitrStarted,
    PitrCompleted,
    PitrFailed,
    PitrGap,

    // Retention events
    RetentionStarted,
    RetentionCompleted,
    RetentionFailed,

    // HA switchover events
    HaSwitchoverStarted,
    HaSwitchoverCompleted,
    HaSwitchoverFailed,

    // HA failover events
    HaFailoverStarted,
    HaFailoverCompleted,
    HaFailoverFailed,

    // Status events
    StatusWarning,
    StatusCritical,
}

impl Serialize for EventType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EventType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        let lower = raw.trim().to_lowercase();

        // Normalize to dot-separated form when possible.
        let dot = if lower.contains('.') {
            lower.clone()
        } else if lower.contains('_') {
            lower.replace('_', ".")
        } else {
            lower.clone()
        };

        let parsed = match dot.as_str() {
            "backup.started" => EventType::BackupStarted,
            "backup.completed" => EventType::BackupCompleted,
            "backup.failed" => EventType::BackupFailed,
            "restore.started" => EventType::RestoreStarted,
            "restore.completed" => EventType::RestoreCompleted,
            "restore.failed" => EventType::RestoreFailed,
            "pitr.started" => EventType::PitrStarted,
            "pitr.completed" => EventType::PitrCompleted,
            "pitr.failed" => EventType::PitrFailed,
            "pitr.gap" => EventType::PitrGap,
            "retention.started" => EventType::RetentionStarted,
            "retention.completed" => EventType::RetentionCompleted,
            "retention.failed" => EventType::RetentionFailed,
            "ha.switchover.started" => EventType::HaSwitchoverStarted,
            "ha.switchover.completed" => EventType::HaSwitchoverCompleted,
            "ha.switchover.failed" => EventType::HaSwitchoverFailed,
            "ha.failover.started" => EventType::HaFailoverStarted,
            "ha.failover.completed" => EventType::HaFailoverCompleted,
            "ha.failover.failed" => EventType::HaFailoverFailed,
            "status.warning" => EventType::StatusWarning,
            "status.critical" => EventType::StatusCritical,
            _ => {
                // Backward compatibility for variants like BackupFailed / backupfailed.
                let compact: String = lower
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect();

                match compact.as_str() {
                    "backupstarted" => EventType::BackupStarted,
                    "backupcompleted" => EventType::BackupCompleted,
                    "backupfailed" => EventType::BackupFailed,
                    "restorestarted" => EventType::RestoreStarted,
                    "restorecompleted" => EventType::RestoreCompleted,
                    "restorefailed" => EventType::RestoreFailed,
                    "pitrstarted" => EventType::PitrStarted,
                    "pitrcompleted" => EventType::PitrCompleted,
                    "pitrfailed" => EventType::PitrFailed,
                    "pitrgap" => EventType::PitrGap,
                    "retentionstarted" => EventType::RetentionStarted,
                    "retentioncompleted" => EventType::RetentionCompleted,
                    "retentionfailed" => EventType::RetentionFailed,
                    "haswitchoverstarted" => EventType::HaSwitchoverStarted,
                    "haswitchovercompleted" => EventType::HaSwitchoverCompleted,
                    "haswitchoverfailed" => EventType::HaSwitchoverFailed,
                    "hafailoverstarted" => EventType::HaFailoverStarted,
                    "hafailovercompleted" => EventType::HaFailoverCompleted,
                    "hafailoverfailed" => EventType::HaFailoverFailed,
                    "statuswarning" => EventType::StatusWarning,
                    "statuscritical" => EventType::StatusCritical,
                    _ => return Err(D::Error::custom(format!("Unknown event type: {raw}"))),
                }
            }
        };

        Ok(parsed)
    }
}

impl EventType {
    /// Get the string representation for event matching.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::BackupStarted => "backup.started",
            EventType::BackupCompleted => "backup.completed",
            EventType::BackupFailed => "backup.failed",
            EventType::RestoreStarted => "restore.started",
            EventType::RestoreCompleted => "restore.completed",
            EventType::RestoreFailed => "restore.failed",
            EventType::PitrStarted => "pitr.started",
            EventType::PitrCompleted => "pitr.completed",
            EventType::PitrFailed => "pitr.failed",
            EventType::PitrGap => "pitr.gap",
            EventType::RetentionStarted => "retention.started",
            EventType::RetentionCompleted => "retention.completed",
            EventType::RetentionFailed => "retention.failed",
            EventType::HaSwitchoverStarted => "ha.switchover.started",
            EventType::HaSwitchoverCompleted => "ha.switchover.completed",
            EventType::HaSwitchoverFailed => "ha.switchover.failed",
            EventType::HaFailoverStarted => "ha.failover.started",
            EventType::HaFailoverCompleted => "ha.failover.completed",
            EventType::HaFailoverFailed => "ha.failover.failed",
            EventType::StatusWarning => "status.warning",
            EventType::StatusCritical => "status.critical",
        }
    }

    /// Get the event category.
    pub fn category(&self) -> EventCategory {
        match self {
            EventType::BackupStarted | EventType::BackupCompleted | EventType::BackupFailed => {
                EventCategory::Backup
            }
            EventType::RestoreStarted | EventType::RestoreCompleted | EventType::RestoreFailed => {
                EventCategory::Restore
            }
            EventType::PitrStarted
            | EventType::PitrCompleted
            | EventType::PitrFailed
            | EventType::PitrGap => EventCategory::Pitr,
            EventType::RetentionStarted
            | EventType::RetentionCompleted
            | EventType::RetentionFailed => EventCategory::Retention,
            EventType::HaSwitchoverStarted
            | EventType::HaSwitchoverCompleted
            | EventType::HaSwitchoverFailed
            | EventType::HaFailoverStarted
            | EventType::HaFailoverCompleted
            | EventType::HaFailoverFailed => EventCategory::Ha,
            EventType::StatusWarning | EventType::StatusCritical => EventCategory::Status,
        }
    }

    /// Get the default severity for this event type.
    pub fn default_severity(&self) -> EventSeverity {
        match self {
            EventType::BackupStarted
            | EventType::RestoreStarted
            | EventType::PitrStarted
            | EventType::RetentionStarted
            | EventType::HaSwitchoverStarted
            | EventType::HaFailoverStarted => EventSeverity::Info,

            EventType::BackupCompleted
            | EventType::RestoreCompleted
            | EventType::PitrCompleted
            | EventType::RetentionCompleted
            | EventType::HaSwitchoverCompleted
            | EventType::HaFailoverCompleted => EventSeverity::Info,

            EventType::StatusWarning | EventType::PitrGap => EventSeverity::Warning,

            EventType::BackupFailed
            | EventType::RestoreFailed
            | EventType::PitrFailed
            | EventType::RetentionFailed
            | EventType::HaSwitchoverFailed
            | EventType::HaFailoverFailed
            | EventType::StatusCritical => EventSeverity::Critical,
        }
    }

    /// Check if this is a failure event.
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            EventType::BackupFailed
                | EventType::RestoreFailed
                | EventType::PitrFailed
                | EventType::RetentionFailed
                | EventType::HaSwitchoverFailed
                | EventType::HaFailoverFailed
        )
    }

    /// Check if this is a success/completion event.
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            EventType::BackupCompleted
                | EventType::RestoreCompleted
                | EventType::PitrCompleted
                | EventType::RetentionCompleted
                | EventType::HaSwitchoverCompleted
                | EventType::HaFailoverCompleted
        )
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Event payload with operation-specific details.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventPayload {
    /// Backup event details
    Backup(BackupEventPayload),
    /// Restore event details
    Restore(RestoreEventPayload),
    /// PITR event details
    Pitr(PitrEventPayload),
    /// Retention event details
    Retention(RetentionEventPayload),
    /// HA event details
    Ha(HaEventPayload),
    /// Status event details
    Status(StatusEventPayload),
    /// Generic/custom payload
    Generic(HashMap<String, serde_json::Value>),
}

/// Backup event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEventPayload {
    /// Backup ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    /// Database name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// Host
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Backup type (full, incremental, snapshot)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_type: Option<String>,
    /// Backup size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// Local path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    /// Remote path (S3 key)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<String>,
    /// Error message (for failed events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Restore event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreEventPayload {
    /// Backup ID being restored
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    /// Target directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_dir: Option<String>,
    /// Database name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// Error message (for failed events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// PITR event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitrEventPayload {
    /// Base backup ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
    /// Target time for recovery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_time: Option<DateTime<Utc>>,
    /// Target directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_dir: Option<String>,
    /// WAL segments applied
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wal_segments_applied: Option<u32>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// Gap start time (for pitr.gap events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_start: Option<DateTime<Utc>>,
    /// Gap end time (for pitr.gap events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_end: Option<DateTime<Utc>>,
    /// Error message (for failed events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Retention event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionEventPayload {
    /// Number of backups evaluated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backups_evaluated: Option<u32>,
    /// Number of backups deleted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backups_deleted: Option<u32>,
    /// Space reclaimed in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_reclaimed_bytes: Option<u64>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// Error message (for failed events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// HA event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaEventPayload {
    /// Cluster ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    /// Operation type (switchover, failover)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Source node ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_node: Option<String>,
    /// Target node ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_node: Option<String>,
    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// Data loss estimate (for failover)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_loss_estimate: Option<String>,
    /// Error message (for failed events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Status event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEventPayload {
    /// Status message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Component affected
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// Additional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, serde_json::Value>>,
}

/// A complete event with all metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Event ID (UUID)
    pub id: String,

    /// Event type
    pub event_type: EventType,

    /// Event severity
    pub severity: EventSeverity,

    /// Event timestamp
    pub timestamp: DateTime<Utc>,

    /// Agent/host identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// Hostname
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,

    /// Human-readable message
    pub message: String,

    /// Event payload with details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<EventPayload>,

    /// Additional labels/tags
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
}

impl Event {
    /// Create a new event.
    pub fn new(event_type: EventType, message: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            severity: event_type.default_severity(),
            event_type,
            timestamp: Utc::now(),
            agent_id: None,
            hostname: hostname::get().ok().and_then(|h| h.into_string().ok()),
            message: message.into(),
            payload: None,
            labels: HashMap::new(),
        }
    }

    /// Set the event payload.
    pub fn with_payload(mut self, payload: EventPayload) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Set the agent ID.
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.agent_id = Some(agent_id.into());
        self
    }

    /// Set the severity.
    pub fn with_severity(mut self, severity: EventSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Add a label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Add multiple labels.
    pub fn with_labels(mut self, labels: HashMap<String, String>) -> Self {
        self.labels.extend(labels);
        self
    }

    /// Get the event type string for pattern matching.
    pub fn event_type_str(&self) -> &'static str {
        self.event_type.as_str()
    }
}

/// Builder for creating backup events.
#[allow(dead_code)] // Will be used for structured event creation in daemon/CLI
pub struct BackupEventBuilder {
    event_type: EventType,
    message: String,
    payload: BackupEventPayload,
    labels: HashMap<String, String>,
}

#[allow(dead_code)] // Will be used for structured event creation in daemon/CLI
impl BackupEventBuilder {
    pub fn started() -> Self {
        Self {
            event_type: EventType::BackupStarted,
            message: "Backup started".to_string(),
            payload: BackupEventPayload {
                backup_id: None,
                database: None,
                host: None,
                backup_type: None,
                size_bytes: None,
                duration_secs: None,
                local_path: None,
                remote_path: None,
                error: None,
            },
            labels: HashMap::new(),
        }
    }

    pub fn completed() -> Self {
        Self {
            event_type: EventType::BackupCompleted,
            message: "Backup completed successfully".to_string(),
            payload: BackupEventPayload {
                backup_id: None,
                database: None,
                host: None,
                backup_type: None,
                size_bytes: None,
                duration_secs: None,
                local_path: None,
                remote_path: None,
                error: None,
            },
            labels: HashMap::new(),
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        let error_str = error.into();
        Self {
            event_type: EventType::BackupFailed,
            message: format!("Backup failed: {}", error_str),
            payload: BackupEventPayload {
                backup_id: None,
                database: None,
                host: None,
                backup_type: None,
                size_bytes: None,
                duration_secs: None,
                local_path: None,
                remote_path: None,
                error: Some(error_str),
            },
            labels: HashMap::new(),
        }
    }

    pub fn backup_id(mut self, id: impl Into<String>) -> Self {
        self.payload.backup_id = Some(id.into());
        self
    }

    pub fn database(mut self, db: impl Into<String>) -> Self {
        self.payload.database = Some(db.into());
        self
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.payload.host = Some(host.into());
        self
    }

    pub fn backup_type(mut self, backup_type: impl Into<String>) -> Self {
        self.payload.backup_type = Some(backup_type.into());
        self
    }

    pub fn size_bytes(mut self, size: u64) -> Self {
        self.payload.size_bytes = Some(size);
        self
    }

    pub fn duration_secs(mut self, duration: f64) -> Self {
        self.payload.duration_secs = Some(duration);
        self
    }

    pub fn local_path(mut self, path: impl Into<String>) -> Self {
        self.payload.local_path = Some(path.into());
        self
    }

    pub fn remote_path(mut self, path: impl Into<String>) -> Self {
        self.payload.remote_path = Some(path.into());
        self
    }

    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> Event {
        Event::new(self.event_type, self.message)
            .with_payload(EventPayload::Backup(self.payload))
            .with_labels(self.labels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_string() {
        assert_eq!(EventType::BackupStarted.as_str(), "backup.started");
        assert_eq!(
            EventType::HaFailoverCompleted.as_str(),
            "ha.failover.completed"
        );
        assert_eq!(EventType::PitrGap.as_str(), "pitr.gap");
    }

    #[test]
    fn test_event_category() {
        assert_eq!(EventType::BackupFailed.category(), EventCategory::Backup);
        assert_eq!(EventType::HaSwitchoverStarted.category(), EventCategory::Ha);
        assert_eq!(EventType::StatusWarning.category(), EventCategory::Status);
    }

    #[test]
    fn test_event_severity() {
        assert_eq!(
            EventType::BackupStarted.default_severity(),
            EventSeverity::Info
        );
        assert_eq!(
            EventType::BackupFailed.default_severity(),
            EventSeverity::Critical
        );
        assert_eq!(
            EventType::PitrGap.default_severity(),
            EventSeverity::Warning
        );
    }

    #[test]
    fn test_backup_event_builder() {
        let event = BackupEventBuilder::completed()
            .backup_id("backup-123")
            .database("mydb")
            .host("localhost")
            .backup_type("snapshot")
            .size_bytes(1024 * 1024)
            .duration_secs(30.5)
            .build();

        assert_eq!(event.event_type, EventType::BackupCompleted);
        assert_eq!(event.severity, EventSeverity::Info);
        assert!(event.message.contains("completed"));

        if let Some(EventPayload::Backup(payload)) = &event.payload {
            assert_eq!(payload.backup_id, Some("backup-123".to_string()));
            assert_eq!(payload.database, Some("mydb".to_string()));
            assert_eq!(payload.size_bytes, Some(1024 * 1024));
        } else {
            panic!("Expected backup payload");
        }
    }

    #[test]
    fn test_event_serialization() {
        let event =
            Event::new(EventType::BackupFailed, "Test failure").with_label("env", "production");

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("backup.failed") || json.contains("BackupFailed"));
        assert!(json.contains("Test failure"));
    }
}
