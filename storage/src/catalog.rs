//! Backup catalog module for listing, inspecting, and downloading backups from S3-compatible storage.
//!
//! This module provides offline-first backup catalog operations that work entirely
//! with local CLI and S3 storage, without requiring HOLD or C2 connections.

use crate::{
    BackupMetadata, BackupStatus, BackupType, PostgresBackupStorage, StorageError, StorageObject,
};
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Filter criteria for listing backups
#[derive(Debug, Clone, Default)]
pub struct BackupFilter {
    /// Filter by backup type
    pub backup_type: Option<BackupType>,
    /// Filter by database name
    pub database: Option<String>,
    /// Filter by status
    pub status: Option<BackupStatus>,
    /// Filter by minimum timestamp (inclusive)
    pub after: Option<DateTime<Utc>>,
    /// Filter by maximum timestamp (inclusive)
    pub before: Option<DateTime<Utc>>,
    /// Filter by labels (all must match)
    pub labels: HashMap<String, String>,
    /// Maximum number of results to return
    pub limit: Option<usize>,
}

impl BackupFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_backup_type(mut self, backup_type: BackupType) -> Self {
        self.backup_type = Some(backup_type);
        self
    }

    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    pub fn with_status(mut self, status: BackupStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn after(mut self, timestamp: DateTime<Utc>) -> Self {
        self.after = Some(timestamp);
        self
    }

    pub fn before(mut self, timestamp: DateTime<Utc>) -> Self {
        self.before = Some(timestamp);
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Check if a backup matches this filter
    pub fn matches(&self, backup: &BackupMetadata) -> bool {
        // Check backup type
        if let Some(ref bt) = self.backup_type {
            if &backup.backup_type != bt {
                return false;
            }
        }

        // Check status
        if let Some(ref status) = self.status {
            if &backup.status != status {
                return false;
            }
        }

        // Check after timestamp
        if let Some(ref after) = self.after {
            if backup.start_time < *after {
                return false;
            }
        }

        // Check before timestamp
        if let Some(ref before) = self.before {
            if backup.start_time > *before {
                return false;
            }
        }

        // Check labels (all must match)
        for (key, value) in &self.labels {
            let label_str = format!("{}={}", key, value);
            if !backup.tags.contains(&label_str) {
                return false;
            }
        }

        true
    }
}

/// Summary information for a backup in list view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSummary {
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
    /// Total size in bytes
    pub size_bytes: u64,
    /// PostgreSQL server version
    pub server_version: String,
    /// Number of files in the backup
    pub file_count: usize,
    /// Tags/labels
    pub tags: Vec<String>,
    /// Whether this backup is pinned
    pub pinned: bool,
    /// Storage location (bucket/prefix)
    pub storage_location: String,
}

impl From<&BackupMetadata> for BackupSummary {
    fn from(metadata: &BackupMetadata) -> Self {
        Self {
            id: metadata.id.clone(),
            backup_type: metadata.backup_type.clone(),
            status: metadata.status,
            start_time: metadata.start_time,
            end_time: metadata.end_time,
            size_bytes: metadata.size_bytes,
            server_version: metadata.server_version.clone(),
            file_count: metadata.files.len(),
            tags: metadata.tags.clone(),
            pinned: metadata.pinned,
            storage_location: String::new(), // Will be set by caller
        }
    }
}

/// Detailed backup information for inspection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupDetails {
    /// Full backup metadata
    pub metadata: BackupMetadata,
    /// Storage bucket
    pub bucket: String,
    /// Storage prefix/key
    pub storage_key: String,
    /// List of all objects in this backup
    pub objects: Vec<BackupObject>,
    /// Total storage size (may differ from metadata due to compression)
    pub total_storage_size: u64,
}

/// Information about a single object in a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupObject {
    /// Object key in storage
    pub key: String,
    /// Size in bytes
    pub size: u64,
    /// Last modified time
    pub last_modified: Option<DateTime<Utc>>,
    /// ETag (usually MD5 hash)
    pub etag: Option<String>,
}

impl From<&StorageObject> for BackupObject {
    fn from(obj: &StorageObject) -> Self {
        Self {
            key: obj.key.clone(),
            size: obj.size.unwrap_or(0),
            last_modified: obj.last_modified,
            etag: obj.etag.clone(),
        }
    }
}

/// Result of a download operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadResult {
    /// Backup ID that was downloaded
    pub backup_id: String,
    /// Target directory where files were downloaded
    pub target_dir: String,
    /// Number of files downloaded
    pub files_downloaded: usize,
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
    /// Duration of download in seconds
    pub duration_secs: u64,
    /// Checksum verification result (if requested)
    pub checksum_verified: Option<ChecksumResult>,
}

/// Result of checksum verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumResult {
    /// Whether all checksums matched
    pub all_matched: bool,
    /// Number of files verified
    pub files_verified: usize,
    /// Number of files with matching checksums
    pub files_matched: usize,
    /// Number of files with mismatched checksums
    pub files_mismatched: usize,
    /// Number of files skipped (no checksum in metadata)
    pub files_skipped: usize,
    /// Details of any mismatches
    pub mismatches: Vec<ChecksumMismatch>,
}

/// Details of a checksum mismatch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumMismatch {
    /// File name
    pub file: String,
    /// Expected checksum from metadata
    pub expected: String,
    /// Actual checksum computed from downloaded file
    pub actual: String,
}

/// Backup catalog operations
impl PostgresBackupStorage {
    /// List backups with optional filtering
    pub async fn list_backups_filtered(
        &self,
        filter: &BackupFilter,
    ) -> Result<Vec<BackupSummary>, StorageError> {
        info!("Listing backups with filter: {:?}", filter);

        // Get all detailed backup metadata
        let all_backups = self.list_remote_backups_detailed().await?;

        // Apply filters
        let mut filtered: Vec<BackupSummary> = all_backups
            .iter()
            .filter(|b| filter.matches(b))
            .map(|b| {
                let mut summary = BackupSummary::from(b);
                summary.storage_location = self.get_storage_location();
                summary
            })
            .collect();

        // Sort by start_time descending (newest first)
        filtered.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        // Apply limit if specified
        if let Some(limit) = filter.limit {
            filtered.truncate(limit);
        }

        info!("Found {} backups matching filter", filtered.len());
        Ok(filtered)
    }

    /// Get detailed information about a specific backup
    pub async fn get_backup_details(
        &self,
        backup_id: &str,
    ) -> Result<BackupDetails, StorageError> {
        info!("Getting details for backup: {}", backup_id);

        // Get metadata
        let metadata = self.get_remote_backup_metadata(backup_id).await?;

        // List all objects for this backup
        let backup_prefix = self.get_backup_prefix(backup_id);
        let objects = self.list_backup_objects(backup_id).await?;

        let total_storage_size: u64 = objects.iter().map(|o| o.size).sum();

        Ok(BackupDetails {
            metadata,
            bucket: self.get_bucket().to_string(),
            storage_key: backup_prefix,
            objects,
            total_storage_size,
        })
    }

    /// List all objects belonging to a specific backup
    pub async fn list_backup_objects(
        &self,
        backup_id: &str,
    ) -> Result<Vec<BackupObject>, StorageError> {
        let backup_prefix = self.get_backup_prefix(backup_id);

        let objects = self
            .list_objects_with_prefix(&backup_prefix)
            .await?;

        Ok(objects.iter().map(BackupObject::from).collect())
    }

    /// Download a backup with optional checksum verification
    pub async fn download_backup_verified(
        &self,
        backup_id: &str,
        target_dir: &Path,
        verify_checksums: bool,
    ) -> Result<DownloadResult, StorageError> {
        let start_time = std::time::Instant::now();
        info!(
            "Downloading backup {} to {} (verify_checksums={})",
            backup_id,
            target_dir.display(),
            verify_checksums
        );

        // Get metadata first (to verify backup exists and for checksum verification)
        let metadata = self.get_remote_backup_metadata(backup_id).await?;

        // Download the backup
        self.download_backup(backup_id, target_dir).await?;

        // Count downloaded files and bytes
        let (files_downloaded, bytes_downloaded) = count_directory_contents(target_dir)?;

        // Verify checksums if requested
        let checksum_verified = if verify_checksums {
            Some(verify_backup_checksums(target_dir, &metadata)?)
        } else {
            None
        };

        let duration_secs = start_time.elapsed().as_secs();

        let result = DownloadResult {
            backup_id: backup_id.to_string(),
            target_dir: target_dir.to_string_lossy().to_string(),
            files_downloaded,
            bytes_downloaded,
            duration_secs,
            checksum_verified,
        };

        info!(
            "Download complete: {} files, {} bytes in {}s",
            result.files_downloaded, result.bytes_downloaded, result.duration_secs
        );

        Ok(result)
    }

    /// Check if a backup exists in remote storage
    pub async fn backup_exists(&self, backup_id: &str) -> Result<bool, StorageError> {
        let metadata_key = self.get_metadata_key(backup_id);
        self.object_exists_at_key(&metadata_key).await
    }

    // Helper methods

    fn get_storage_location(&self) -> String {
        let bucket = self.get_bucket();
        let prefix = self.get_prefix();
        if prefix.is_empty() {
            format!("s3://{}", bucket)
        } else {
            format!("s3://{}/{}", bucket, prefix)
        }
    }

    fn get_backup_prefix(&self, backup_id: &str) -> String {
        let prefix = self.get_prefix();
        if prefix.is_empty() {
            backup_id.to_string()
        } else {
            format!("{}/{}", prefix, backup_id)
        }
    }

    fn get_metadata_key(&self, backup_id: &str) -> String {
        let prefix = self.get_prefix();
        if prefix.is_empty() {
            format!("{}/backup_metadata.json", backup_id)
        } else {
            format!("{}/{}/backup_metadata.json", prefix, backup_id)
        }
    }

    /// Get the bucket name
    pub fn get_bucket(&self) -> &str {
        &self.bucket
    }

    /// Get the prefix
    pub fn get_prefix(&self) -> &str {
        &self.prefix
    }

    /// List objects with a specific prefix
    async fn list_objects_with_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<StorageObject>, StorageError> {
        self.provider.list_objects(&self.bucket, Some(prefix)).await
    }

    /// Check if an object exists at a specific key
    async fn object_exists_at_key(&self, key: &str) -> Result<bool, StorageError> {
        self.provider.object_exists(&self.bucket, key).await
    }
}

/// Count files and total bytes in a directory
fn count_directory_contents(path: &Path) -> Result<(usize, u64), StorageError> {
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;

    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            file_count += 1;
            total_bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }

    Ok((file_count, total_bytes))
}

/// Verify checksums of downloaded files against metadata
fn verify_backup_checksums(
    target_dir: &Path,
    metadata: &BackupMetadata,
) -> Result<ChecksumResult, StorageError> {
    info!("Verifying checksums for {} files", metadata.files.len());

    let mut files_verified = 0usize;
    let mut files_matched = 0usize;
    let mut files_mismatched = 0usize;
    let mut files_skipped = 0usize;
    let mut mismatches = Vec::new();

    for file_info in &metadata.files {
        let file_path = target_dir.join(&file_info.name);

        if !file_path.exists() {
            warn!("File not found for checksum verification: {}", file_info.name);
            files_skipped += 1;
            continue;
        }

        match &file_info.checksum {
            Some(expected_checksum) => {
                files_verified += 1;

                match compute_file_checksum(&file_path) {
                    Ok(actual_checksum) => {
                        if actual_checksum == *expected_checksum {
                            files_matched += 1;
                        } else {
                            files_mismatched += 1;
                            mismatches.push(ChecksumMismatch {
                                file: file_info.name.clone(),
                                expected: expected_checksum.clone(),
                                actual: actual_checksum,
                            });
                            error!(
                                "Checksum mismatch for {}: expected {}, got {}",
                                file_info.name, expected_checksum, mismatches.last().unwrap().actual
                            );
                        }
                    }
                    Err(e) => {
                        error!("Failed to compute checksum for {}: {}", file_info.name, e);
                        files_skipped += 1;
                    }
                }
            }
            None => {
                files_skipped += 1;
            }
        }
    }

    let all_matched = files_mismatched == 0;

    info!(
        "Checksum verification complete: {} verified, {} matched, {} mismatched, {} skipped",
        files_verified, files_matched, files_mismatched, files_skipped
    );

    Ok(ChecksumResult {
        all_matched,
        files_verified,
        files_matched,
        files_mismatched,
        files_skipped,
        mismatches,
    })
}

/// Compute SHA256 checksum of a file
fn compute_file_checksum(path: &Path) -> Result<String, StorageError> {
    let mut file = std::fs::File::open(path).map_err(StorageError::Io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 8192];

    loop {
        let n = file.read(&mut buffer).map_err(StorageError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backup_filter_default() {
        let filter = BackupFilter::new();
        assert!(filter.backup_type.is_none());
        assert!(filter.database.is_none());
        assert!(filter.status.is_none());
        assert!(filter.after.is_none());
        assert!(filter.before.is_none());
        assert!(filter.labels.is_empty());
        assert!(filter.limit.is_none());
    }

    #[test]
    fn test_backup_filter_builder() {
        let filter = BackupFilter::new()
            .with_backup_type(BackupType::Snapshot)
            .with_database("mydb")
            .with_status(BackupStatus::Completed)
            .with_label("env", "prod")
            .with_limit(10);

        assert_eq!(filter.backup_type, Some(BackupType::Snapshot));
        assert_eq!(filter.database, Some("mydb".to_string()));
        assert_eq!(filter.status, Some(BackupStatus::Completed));
        assert_eq!(filter.labels.get("env"), Some(&"prod".to_string()));
        assert_eq!(filter.limit, Some(10));
    }

    #[test]
    fn test_backup_filter_matches() {
        let metadata = BackupMetadata {
            id: "test-123".to_string(),
            backup_type: BackupType::Snapshot,
            status: BackupStatus::Completed,
            start_time: Utc::now(),
            end_time: Some(Utc::now()),
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 1000,
            server_version: "15.0".to_string(),
            checksum: None,
            files: vec![],
            tags: vec!["env=prod".to_string(), "app=billing".to_string()],
            pinned: false,
            encrypted: None,
            encryption_algorithm: None,
        };

        // Empty filter matches everything
        let filter = BackupFilter::new();
        assert!(filter.matches(&metadata));

        // Matching backup type
        let filter = BackupFilter::new().with_backup_type(BackupType::Snapshot);
        assert!(filter.matches(&metadata));

        // Non-matching backup type
        let filter = BackupFilter::new().with_backup_type(BackupType::Full);
        assert!(!filter.matches(&metadata));

        // Matching label
        let filter = BackupFilter::new().with_label("env", "prod");
        assert!(filter.matches(&metadata));

        // Non-matching label
        let filter = BackupFilter::new().with_label("env", "staging");
        assert!(!filter.matches(&metadata));

        // Multiple matching labels
        let filter = BackupFilter::new()
            .with_label("env", "prod")
            .with_label("app", "billing");
        assert!(filter.matches(&metadata));
    }

    #[test]
    fn test_backup_summary_from_metadata() {
        let metadata = BackupMetadata {
            id: "test-123".to_string(),
            backup_type: BackupType::Snapshot,
            status: BackupStatus::Completed,
            start_time: Utc::now(),
            end_time: Some(Utc::now()),
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 1000,
            server_version: "15.0".to_string(),
            checksum: None,
            files: vec![],
            tags: vec!["env=prod".to_string()],
            pinned: true,
            encrypted: None,
            encryption_algorithm: None,
        };

        let summary = BackupSummary::from(&metadata);
        assert_eq!(summary.id, "test-123");
        assert_eq!(summary.backup_type, BackupType::Snapshot);
        assert_eq!(summary.status, BackupStatus::Completed);
        assert_eq!(summary.size_bytes, 1000);
        assert_eq!(summary.server_version, "15.0");
        assert!(summary.pinned);
    }
}
