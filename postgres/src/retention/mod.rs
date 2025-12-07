//! Backup retention and purge policies for PostgreSQL backups.
//!
//! This module implements PITR-aware retention policies that ensure:
//! - Backup chains are preserved (incrementals depend on their base)
//! - WAL segments required for PITR windows are retained
//! - Configurable retention rules (time-based, count-based, interval-based)
//! - Safe purge operations with dry-run support

mod engine;
pub mod policy;
mod wal;

pub use engine::{RetentionEngine, RetentionEvaluation, RetentionDecision};
pub use policy::{
    PitrRetentionPolicy, RetentionRule, RetentionScope, WalRetentionConfig, SafetySettings,
};
pub use wal::{WalSegment, WalInventory};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a backup item for retention evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupItem {
    /// Unique backup identifier
    pub id: String,
    /// Type of backup (full, incremental, snapshot)
    pub backup_type: BackupItemType,
    /// Backup status
    pub status: BackupItemStatus,
    /// When the backup started
    pub start_time: DateTime<Utc>,
    /// When the backup completed
    pub end_time: Option<DateTime<Utc>>,
    /// Base backup ID for incremental backups
    pub base_backup_id: Option<String>,
    /// WAL start position (LSN)
    pub wal_start: Option<String>,
    /// WAL end position (LSN)
    pub wal_end: Option<String>,
    /// Size in bytes
    pub size_bytes: u64,
    /// Database name
    pub database: Option<String>,
    /// Whether this backup is pinned (never purge)
    pub pinned: bool,
    /// Tags for policy exceptions
    pub tags: Vec<String>,
    /// Location: local path or remote key
    pub location: BackupLocation,
}

/// Type of backup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupItemType {
    Full,
    Incremental,
    Snapshot,
}

/// Status of a backup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupItemStatus {
    InProgress,
    Completed,
    Failed,
}

/// Location of a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackupLocation {
    /// Local filesystem path
    Local(String),
    /// Remote S3 key
    Remote(String),
    /// Both local and remote
    Both { local: String, remote: String },
}

impl BackupItem {
    /// Returns the effective timestamp for sorting (end_time or start_time)
    pub fn effective_time(&self) -> DateTime<Utc> {
        self.end_time.unwrap_or(self.start_time)
    }

    /// Checks if this backup is completed
    pub fn is_completed(&self) -> bool {
        self.status == BackupItemStatus::Completed
    }

    /// Checks if this is a full backup
    pub fn is_full(&self) -> bool {
        self.backup_type == BackupItemType::Full
    }

    /// Checks if this is an incremental backup
    pub fn is_incremental(&self) -> bool {
        self.backup_type == BackupItemType::Incremental
    }
}

/// Result of a retention evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionResult {
    /// When this evaluation was performed
    pub timestamp: DateTime<Utc>,
    /// Total number of backups evaluated
    pub total_backups: usize,
    /// Total number of WAL segments evaluated
    pub total_wal_segments: usize,
    /// Backups to keep
    pub backups_to_keep: Vec<RetentionDecision>,
    /// Backups to delete
    pub backups_to_delete: Vec<RetentionDecision>,
    /// WAL segments to keep
    pub wal_to_keep: Vec<WalRetentionDecision>,
    /// WAL segments to delete
    pub wal_to_delete: Vec<WalRetentionDecision>,
    /// Warnings about the evaluation
    pub warnings: Vec<String>,
    /// Estimated space to be freed (bytes)
    pub estimated_space_freed: u64,
    /// PITR window preserved (earliest recoverable time)
    pub pitr_window_start: Option<DateTime<Utc>>,
    /// PITR window end (latest recoverable time)
    pub pitr_window_end: Option<DateTime<Utc>>,
}

/// Decision for a WAL segment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalRetentionDecision {
    /// WAL segment name (e.g., 000000010000000000000001)
    pub segment_name: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// Reason for keeping or deleting
    pub reason: String,
    /// Location of the segment
    pub location: BackupLocation,
    /// Timeline ID
    pub timeline: u32,
}

/// Report of a purge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurgeReport {
    /// When the purge was executed
    pub timestamp: DateTime<Utc>,
    /// Whether this was a dry run
    pub dry_run: bool,
    /// Total backups evaluated
    pub total_backups_evaluated: usize,
    /// Total WAL segments evaluated
    pub total_wal_evaluated: usize,
    /// Number of backups kept
    pub backups_kept: usize,
    /// Number of backups deleted
    pub backups_deleted: usize,
    /// Number of WAL segments kept
    pub wal_kept: usize,
    /// Number of WAL segments deleted
    pub wal_deleted: usize,
    /// Number of failed deletions
    pub failed: usize,
    /// Space freed in bytes
    pub space_freed: u64,
    /// Duration of purge operation in seconds
    pub duration_secs: u64,
    /// Errors encountered
    pub errors: Vec<String>,
    /// PITR window after purge
    pub pitr_window_start: Option<DateTime<Utc>>,
    pub pitr_window_end: Option<DateTime<Utc>>,
}

impl RetentionResult {
    /// Creates a new empty result
    pub fn new() -> Self {
        Self {
            timestamp: Utc::now(),
            total_backups: 0,
            total_wal_segments: 0,
            backups_to_keep: Vec::new(),
            backups_to_delete: Vec::new(),
            wal_to_keep: Vec::new(),
            wal_to_delete: Vec::new(),
            warnings: Vec::new(),
            estimated_space_freed: 0,
            pitr_window_start: None,
            pitr_window_end: None,
        }
    }

    /// Calculates total space to be freed
    pub fn calculate_space_freed(&mut self) {
        self.estimated_space_freed = self.backups_to_delete.iter().map(|d| d.size_bytes).sum::<u64>()
            + self.wal_to_delete.iter().map(|d| d.size_bytes).sum::<u64>();
    }
}

impl Default for RetentionResult {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_item_effective_time() {
        let now = Utc::now();
        let later = now + chrono::Duration::hours(1);

        let backup = BackupItem {
            id: "test".to_string(),
            backup_type: BackupItemType::Full,
            status: BackupItemStatus::Completed,
            start_time: now,
            end_time: Some(later),
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 1000,
            database: Some("testdb".to_string()),
            pinned: false,
            tags: vec![],
            location: BackupLocation::Local("/backups/test".to_string()),
        };

        assert_eq!(backup.effective_time(), later);
    }

    #[test]
    fn test_backup_item_without_end_time() {
        let now = Utc::now();

        let backup = BackupItem {
            id: "test".to_string(),
            backup_type: BackupItemType::Full,
            status: BackupItemStatus::InProgress,
            start_time: now,
            end_time: None,
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 0,
            database: Some("testdb".to_string()),
            pinned: false,
            tags: vec![],
            location: BackupLocation::Local("/backups/test".to_string()),
        };

        assert_eq!(backup.effective_time(), now);
    }
}
