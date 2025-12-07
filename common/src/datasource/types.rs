//! Core types for the data source plugin system.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

/// Type of backup operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupType {
    /// Full backup (complete copy of all data)
    Full,
    /// Incremental backup (changes since last backup)
    Incremental,
    /// Snapshot backup (point-in-time logical backup)
    Snapshot,
    /// Differential backup (changes since last full backup)
    Differential,
}

impl std::fmt::Display for BackupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupType::Full => write!(f, "full"),
            BackupType::Incremental => write!(f, "incremental"),
            BackupType::Snapshot => write!(f, "snapshot"),
            BackupType::Differential => write!(f, "differential"),
        }
    }
}

/// Status of a backup operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStatus {
    /// Backup is in progress
    InProgress,
    /// Backup completed successfully
    Completed,
    /// Backup failed
    Failed,
    /// Backup was cancelled
    Cancelled,
}

/// Status of a restore operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreStatus {
    /// Restore is in progress
    InProgress,
    /// Restore completed successfully
    Completed,
    /// Restore failed
    Failed,
    /// Restore was cancelled
    Cancelled,
}

/// Metadata about a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Unique identifier for this backup
    pub backup_id: Uuid,
    /// Type of backup
    pub backup_type: BackupType,
    /// Current status
    pub status: BackupStatus,
    /// When the backup started
    pub start_time: DateTime<Utc>,
    /// When the backup completed (if finished)
    pub end_time: Option<DateTime<Utc>>,
    /// Size of the backup in bytes
    pub size_bytes: Option<u64>,
    /// Local path to backup files
    pub local_path: Option<PathBuf>,
    /// Remote storage path (if uploaded)
    pub remote_path: Option<String>,
    /// Server version at time of backup
    pub server_version: Option<String>,
    /// Data source type (e.g., "postgresql", "mysql")
    pub datasource_type: String,
    /// Database name
    pub database: Option<String>,
    /// Custom labels/tags
    pub labels: HashMap<String, String>,
    /// Error message if backup failed
    pub error_message: Option<String>,
    /// For incremental backups, the base backup ID
    pub base_backup_id: Option<Uuid>,
    /// Data source specific metadata
    pub extra: HashMap<String, serde_json::Value>,
}

impl BackupMetadata {
    /// Create new backup metadata
    pub fn new(
        backup_type: BackupType,
        datasource_type: impl Into<String>,
        database: Option<String>,
    ) -> Self {
        Self {
            backup_id: Uuid::new_v4(),
            backup_type,
            status: BackupStatus::InProgress,
            start_time: Utc::now(),
            end_time: None,
            size_bytes: None,
            local_path: None,
            remote_path: None,
            server_version: None,
            datasource_type: datasource_type.into(),
            database,
            labels: HashMap::new(),
            error_message: None,
            base_backup_id: None,
            extra: HashMap::new(),
        }
    }

    /// Mark backup as completed
    pub fn complete(&mut self, size_bytes: u64) {
        self.status = BackupStatus::Completed;
        self.end_time = Some(Utc::now());
        self.size_bytes = Some(size_bytes);
    }

    /// Mark backup as failed
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = BackupStatus::Failed;
        self.end_time = Some(Utc::now());
        self.error_message = Some(error.into());
    }

    /// Get the duration of the backup
    pub fn duration(&self) -> Option<Duration> {
        self.end_time.map(|end| {
            let duration = end - self.start_time;
            Duration::from_secs(duration.num_seconds().max(0) as u64)
        })
    }
}

/// Result of a discovery operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverResult {
    /// Whether the connection was successful
    pub connected: bool,
    /// Server version information
    pub server_version: Option<String>,
    /// Additional server metadata
    pub metadata: HashMap<String, String>,
    /// List of available databases/schemas
    pub databases: Vec<String>,
    /// Connection latency
    pub latency_ms: Option<u64>,
}

/// Result of a backup operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    /// Unique identifier for this backup
    pub backup_id: Uuid,
    /// Type of backup performed
    pub backup_type: BackupType,
    /// Local path to backup files
    pub local_path: PathBuf,
    /// Remote storage path (if uploaded)
    pub remote_path: Option<String>,
    /// Backup metadata
    pub metadata: BackupMetadata,
    /// Duration of the backup operation
    pub duration: Duration,
}

/// Result of a restore operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    /// Unique identifier for this restore operation
    pub restore_id: Uuid,
    /// Backup that was restored
    pub backup_id: Uuid,
    /// Status of the restore
    pub status: RestoreStatus,
    /// Path where data was restored
    pub restore_path: PathBuf,
    /// Duration of the restore operation
    pub duration: Duration,
    /// Whether the service was restarted
    pub service_restarted: bool,
    /// Target time for PITR (if applicable)
    pub target_time: Option<DateTime<Utc>>,
    /// Error message if restore failed
    pub error_message: Option<String>,
}

impl RestoreResult {
    /// Create a new successful restore result
    pub fn success(
        backup_id: Uuid,
        restore_path: PathBuf,
        duration: Duration,
        service_restarted: bool,
    ) -> Self {
        Self {
            restore_id: Uuid::new_v4(),
            backup_id,
            status: RestoreStatus::Completed,
            restore_path,
            duration,
            service_restarted,
            target_time: None,
            error_message: None,
        }
    }

    /// Create a failed restore result
    pub fn failure(backup_id: Uuid, restore_path: PathBuf, error: impl Into<String>) -> Self {
        Self {
            restore_id: Uuid::new_v4(),
            backup_id,
            status: RestoreStatus::Failed,
            restore_path,
            duration: Duration::ZERO,
            service_restarted: false,
            target_time: None,
            error_message: Some(error.into()),
        }
    }
}

/// Status of a data source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceStatus {
    /// Whether the data source is reachable
    pub connected: bool,
    /// Server version
    pub server_version: Option<String>,
    /// Whether the server is accepting connections
    pub accepting_connections: bool,
    /// Current server state (e.g., "primary", "replica", "recovering")
    pub state: Option<String>,
    /// Number of active connections
    pub active_connections: Option<u32>,
    /// Database size in bytes
    pub database_size_bytes: Option<u64>,
    /// Additional status information
    pub extra: HashMap<String, serde_json::Value>,
}

impl DataSourceStatus {
    /// Create a status indicating connection failure
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            server_version: None,
            accepting_connections: false,
            state: None,
            active_connections: None,
            database_size_bytes: None,
            extra: HashMap::new(),
        }
    }

    /// Create a status indicating successful connection
    pub fn connected(server_version: impl Into<String>) -> Self {
        Self {
            connected: true,
            server_version: Some(server_version.into()),
            accepting_connections: true,
            state: None,
            active_connections: None,
            database_size_bytes: None,
            extra: HashMap::new(),
        }
    }
}

/// Describes the capabilities of a data source plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceCapabilities {
    /// Supported backup types
    pub backup_types: Vec<BackupType>,
    /// Whether PITR is supported
    pub supports_pitr: bool,
    /// Whether incremental backups are supported
    pub supports_incremental: bool,
    /// Whether logical backups are supported
    pub supports_logical_backup: bool,
    /// Whether physical backups are supported
    pub supports_physical_backup: bool,
    /// Whether SSH tunneling is supported
    pub supports_ssh_tunnel: bool,
    /// Whether remote storage is supported
    pub supports_remote_storage: bool,
    /// Whether HA/clustering is supported
    pub supports_ha: bool,
    /// Whether the plugin supports encryption
    pub supports_encryption: bool,
    /// Whether the plugin supports compression
    pub supports_compression: bool,
    /// Custom capabilities specific to this data source
    pub custom: HashMap<String, bool>,
}

impl Default for DataSourceCapabilities {
    fn default() -> Self {
        Self {
            backup_types: vec![BackupType::Full],
            supports_pitr: false,
            supports_incremental: false,
            supports_logical_backup: false,
            supports_physical_backup: false,
            supports_ssh_tunnel: false,
            supports_remote_storage: false,
            supports_ha: false,
            supports_encryption: false,
            supports_compression: false,
            custom: HashMap::new(),
        }
    }
}

impl DataSourceCapabilities {
    /// Create capabilities for a full-featured data source
    pub fn full_featured() -> Self {
        Self {
            backup_types: vec![
                BackupType::Full,
                BackupType::Incremental,
                BackupType::Snapshot,
            ],
            supports_pitr: true,
            supports_incremental: true,
            supports_logical_backup: true,
            supports_physical_backup: true,
            supports_ssh_tunnel: true,
            supports_remote_storage: true,
            supports_ha: true,
            supports_encryption: true,
            supports_compression: true,
            custom: HashMap::new(),
        }
    }
}

/// Information about a registered plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Plugin name (e.g., "postgresql")
    pub name: String,
    /// Plugin version
    pub version: String,
    /// Human-readable description
    pub description: String,
    /// Plugin capabilities
    pub capabilities: DataSourceCapabilities,
}

/// Filter for listing backups
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupFilter {
    /// Filter by backup type
    pub backup_type: Option<BackupType>,
    /// Filter by status
    pub status: Option<BackupStatus>,
    /// Filter by database name
    pub database: Option<String>,
    /// Filter backups after this time
    pub after: Option<DateTime<Utc>>,
    /// Filter backups before this time
    pub before: Option<DateTime<Utc>>,
    /// Filter by labels (all must match)
    pub labels: HashMap<String, String>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
}

impl BackupFilter {
    /// Create a new empty filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by backup type
    pub fn with_type(mut self, backup_type: BackupType) -> Self {
        self.backup_type = Some(backup_type);
        self
    }

    /// Filter by status
    pub fn with_status(mut self, status: BackupStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Filter by database
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Limit results
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}
