use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Represents a storage bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    /// Name of the bucket
    pub name: String,
    /// Creation time of the bucket
    pub creation_date: Option<SystemTime>,
    /// Region where the bucket is located
    pub region: Option<String>,
}

/// Represents an object in storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageObject {
    /// Key (path) of the object
    pub key: String,
    /// Size of the object in bytes
    pub size: Option<u64>,
    /// Last modified time
    pub last_modified: Option<DateTime<Utc>>,
    /// ETag of the object
    pub etag: Option<String>,
    /// Storage class of the object
    pub storage_class: Option<String>,
}

/// Represents metadata for an object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadata {
    /// Key (path) of the object
    pub key: String,
    /// Size of the object in bytes
    pub size: Option<u64>,
    /// Last modified time
    pub last_modified: Option<DateTime<Utc>>,
    /// ETag of the object
    pub etag: Option<String>,
    /// Content type of the object
    pub content_type: Option<String>,
    /// Storage class of the object
    pub storage_class: Option<String>,
    /// Custom metadata
    pub metadata: Option<Metadata>,
}

/// Custom metadata for objects
pub type Metadata = HashMap<String, String>;

/// Type of backup
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackupType {
    /// Full backup
    Full,
    /// Incremental backup
    Incremental,
    /// Snapshot backup
    Snapshot,
}

/// Information about a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    /// Backup ID
    pub id: String,
    /// Type of backup
    pub backup_type: BackupType,
    /// Timestamp when the backup was created
    pub timestamp: DateTime<Utc>,
    /// Size of the backup in bytes
    pub size: u64,
    /// Parent backup ID (for incremental backups)
    pub parent_id: Option<String>,
}

/// Storage provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Provider type
    pub provider: StorageProviderType,
    /// Region for the provider
    pub region: Option<String>,
    /// Custom endpoint URL
    pub endpoint: Option<String>,
    /// Access key ID
    pub access_key: Option<String>,
    /// Secret access key
    pub secret_key: Option<String>,
    /// Account ID (for Cloudflare R2)
    pub account_id: Option<String>,
    /// Project ID (for Google Cloud Storage)
    pub project_id: Option<String>,
    /// Path to credentials file (for Google Cloud Storage)
    pub credentials_path: Option<String>,
}

/// Supported storage provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageProviderType {
    /// Amazon S3
    #[serde(rename = "s3")]
    S3,
}

/// Streaming upload options
#[derive(Debug, Clone)]
pub struct StreamingUploadOptions {
    /// Content type of the data
    pub content_type: Option<String>,
    /// Custom metadata
    pub metadata: Option<Metadata>,
    /// Part size for multipart uploads (in bytes)
    pub part_size: Option<usize>,
}

impl Default for StreamingUploadOptions {
    fn default() -> Self {
        Self {
            content_type: None,
            metadata: None,
            part_size: Some(5 * 1024 * 1024), // 5 MB default part size (S3 minimum)
        }
    }
}

/// Streaming download options
#[derive(Debug, Clone, Default)]
pub struct StreamingDownloadOptions {
    /// Range start (in bytes)
    pub range_start: Option<u64>,
    /// Range end (in bytes)
    pub range_end: Option<u64>,
}

/// Status of a backup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupStatus {
    InProgress,
    Completed,
    Failed,
}

/// Detailed metadata for a backup stored in remote storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Backup ID
    pub id: String,
    /// Type of backup
    pub backup_type: BackupType,
    /// Status of the backup
    pub status: BackupStatus,
    /// When the backup started
    pub start_time: DateTime<Utc>,
    /// When the backup completed
    pub end_time: Option<DateTime<Utc>>,
    /// Base backup ID for incremental backups
    pub base_backup_id: Option<String>,
    /// WAL start position
    pub wal_start: Option<String>,
    /// WAL end position
    pub wal_end: Option<String>,
    /// Total size in bytes
    pub size_bytes: u64,
    /// PostgreSQL server version
    pub server_version: String,
    /// Checksum of the backup (SHA256)
    pub checksum: Option<String>,
    /// List of all files in the backup
    pub files: Vec<BackupFile>,
    /// Tags for policy exceptions
    pub tags: Vec<String>,
    /// Whether this backup is pinned (never purge)
    pub pinned: bool,
}

/// Information about a file within a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFile {
    /// File name (relative path within backup)
    pub name: String,
    /// Size in bytes
    pub size: u64,
    /// SHA256 checksum
    pub checksum: Option<String>,
}

/// Retention policy for backups in a bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Policy schema version
    pub version: String,
    /// Whether the policy is enabled
    pub enabled: bool,
    /// The type of retention policy
    pub policy_type: PolicyType,
    /// Safety settings
    pub safety: SafetySettings,
    /// Notification settings
    pub notifications: NotificationSettings,
}

/// Type of retention policy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PolicyType {
    /// Keep backups within time window + minimum count
    TimeBased {
        keep_within_days: u32,
        keep_minimum: usize,
    },
    /// Keep N most recent backups per type
    CountBased {
        max_full_backups: usize,
        max_incrementals_per_full: usize,
        keep_latest: usize,
    },
    /// Interval-based retention (e.g., daily, weekly, monthly, yearly)
    IntervalBased {
        intervals: Vec<RetentionInterval>,
        minimum_backups: usize,
        preserve_chains: bool,
    },
}

/// Retention interval specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionInterval {
    /// Start of this interval (days from now)
    pub after_days: u32,
    /// How many backups to keep in this interval
    pub keep_count: usize,
    /// Spacing between kept backups in days (e.g., 7 for weekly)
    pub spacing_days: u32,
}

/// Safety settings for retention policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySettings {
    /// Perform dry run by default
    pub dry_run_by_default: bool,
    /// Require user confirmation
    pub require_confirmation: bool,
    /// Minimum number of successful backups to keep
    pub min_successful_backups: usize,
    /// Preserve backup chains (don't orphan incrementals)
    pub preserve_chains: bool,
}

/// Notification settings for purge operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSettings {
    /// Report to Sentry
    pub sentry_enabled: bool,
    /// Report errors
    pub report_errors: bool,
    /// Report summary after purge
    pub report_summary: bool,
}

/// Result of evaluating a purge policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeEvaluation {
    /// When this evaluation was performed
    pub timestamp: DateTime<Utc>,
    /// Total number of backups evaluated
    pub total_backups: usize,
    /// Backups to keep
    pub to_keep: Vec<BackupPurgeDecision>,
    /// Backups to delete
    pub to_delete: Vec<BackupPurgeDecision>,
    /// Warnings about the evaluation
    pub warnings: Vec<String>,
    /// Estimated space to be freed (bytes)
    pub estimated_space_freed: u64,
}

/// Decision for a single backup in purge evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPurgeDecision {
    /// Backup ID
    pub backup_id: String,
    /// Type of backup
    pub backup_type: BackupType,
    /// When the backup was created
    pub timestamp: DateTime<Utc>,
    /// Size in bytes
    pub size_bytes: u64,
    /// Reason for keeping or deleting
    pub reason: String,
    /// Whether the backup is pinned
    pub pinned: bool,
    /// Whether this backup has dependent incremental backups
    pub has_dependents: bool,
}

/// Report of a purge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeReport {
    /// When the purge was executed
    pub timestamp: DateTime<Utc>,
    /// Whether this was a dry run
    pub dry_run: bool,
    /// Total backups evaluated
    pub total_evaluated: usize,
    /// Number of backups kept
    pub kept: usize,
    /// Number of backups deleted
    pub deleted: usize,
    /// Number of failed deletions
    pub failed: usize,
    /// Space freed in bytes
    pub space_freed: u64,
    /// Duration of purge operation in seconds
    pub duration_secs: u64,
    /// Errors encountered
    pub errors: Vec<String>,
}
