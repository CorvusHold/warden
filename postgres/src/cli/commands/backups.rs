//! Backup catalog CLI commands for listing, inspecting, and downloading backups.
//!
//! These commands provide offline-first backup management, working entirely
//! with local CLI and S3-compatible storage without requiring HOLD or C2.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use log::{error, info};
use std::path::PathBuf;

use storage::{
    catalog::{BackupDetails, BackupFilter, BackupSummary, DownloadResult},
    BackupStatus as StorageBackupStatus, BackupType as StorageBackupType, PostgresBackupStorage,
    StorageProviderType,
};

/// Options for storage configuration
#[derive(Clone, Debug, Default)]
pub struct BackupStorageOptions {
    pub provider_type: Option<String>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

/// Options for filtering backups in list command
#[derive(Clone, Debug, Default)]
pub struct BackupListOptions {
    pub backup_type: Option<String>,
    pub database: Option<String>,
    pub status: Option<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub labels: Vec<(String, String)>,
    pub limit: Option<usize>,
    pub format: String,
}

/// Result of list backups operation
#[derive(Debug)]
pub struct ListBackupsResult {
    pub backups: Vec<BackupSummary>,
    pub total_count: usize,
    pub total_size_bytes: u64,
}

/// Result of show backup operation
#[derive(Debug)]
pub struct ShowBackupResult {
    pub details: BackupDetails,
}

/// List backups from remote storage with optional filtering
pub async fn list_backups(
    storage_opts: &BackupStorageOptions,
    list_opts: &BackupListOptions,
) -> Result<ListBackupsResult> {
    info!("[backups-list] Starting backup listing");

    let storage = create_storage_provider(storage_opts).await?;

    // Build filter from options
    let mut filter = BackupFilter::new();

    if let Some(ref bt) = list_opts.backup_type {
        filter.backup_type = Some(parse_backup_type(bt)?);
    }

    if let Some(ref db) = list_opts.database {
        filter.database = Some(db.clone());
    }

    if let Some(ref status) = list_opts.status {
        filter.status = Some(parse_backup_status(status)?);
    }

    if let Some(ref after) = list_opts.after {
        filter.after = Some(parse_datetime(after)?);
    }

    if let Some(ref before) = list_opts.before {
        filter.before = Some(parse_datetime(before)?);
    }

    for (key, value) in &list_opts.labels {
        filter.labels.insert(key.clone(), value.clone());
    }

    if let Some(limit) = list_opts.limit {
        filter.limit = Some(limit);
    }

    // Execute the listing
    let backups = storage
        .list_backups_filtered(&filter)
        .await
        .map_err(|e| anyhow!("Failed to list backups: {}", e))?;

    let total_count = backups.len();
    let total_size_bytes: u64 = backups.iter().map(|b| b.size_bytes).sum();

    info!(
        "[backups-list] Found {} backups, total size: {} bytes",
        total_count, total_size_bytes
    );

    Ok(ListBackupsResult {
        backups,
        total_count,
        total_size_bytes,
    })
}

/// Show detailed information about a specific backup
pub async fn show_backup(
    storage_opts: &BackupStorageOptions,
    backup_id: &str,
) -> Result<ShowBackupResult> {
    info!("[backups-show] Getting details for backup: {}", backup_id);

    let storage = create_storage_provider(storage_opts).await?;

    // Check if backup exists
    let exists = storage
        .backup_exists(backup_id)
        .await
        .map_err(|e| anyhow!("Failed to check backup existence: {}", e))?;

    if !exists {
        return Err(anyhow!(
            "Backup '{}' not found in storage bucket '{}'",
            backup_id,
            storage_opts.bucket.as_deref().unwrap_or("unknown")
        ));
    }

    // Get detailed information
    let details = storage
        .get_backup_details(backup_id)
        .await
        .map_err(|e| anyhow!("Failed to get backup details: {}", e))?;

    info!(
        "[backups-show] Backup {} has {} objects, {} bytes total",
        backup_id,
        details.objects.len(),
        details.total_storage_size
    );

    Ok(ShowBackupResult { details })
}

/// Download a backup from remote storage
pub async fn download_backup(
    storage_opts: &BackupStorageOptions,
    backup_id: &str,
    target_dir: &PathBuf,
    verify_checksums: bool,
) -> Result<DownloadResult> {
    info!(
        "[backups-download] Downloading backup {} to {}",
        backup_id,
        target_dir.display()
    );

    let storage = create_storage_provider(storage_opts).await?;

    // Check if backup exists
    let exists = storage
        .backup_exists(backup_id)
        .await
        .map_err(|e| anyhow!("Failed to check backup existence: {}", e))?;

    if !exists {
        return Err(anyhow!(
            "Backup '{}' not found in storage bucket '{}'",
            backup_id,
            storage_opts.bucket.as_deref().unwrap_or("unknown")
        ));
    }

    let mut temp_dir = target_dir.clone();
    temp_dir.set_extension(format!("partial_{}", std::process::id()));

    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).with_context(|| {
            format!(
                "Failed to remove existing temporary download directory: {}",
                temp_dir.display()
            )
        })?;
    }

    if target_dir.exists() {
        let mut entries = std::fs::read_dir(target_dir).with_context(|| {
            format!("Failed to read target directory: {}", target_dir.display())
        })?;

        if entries.next().is_some() {
            return Err(anyhow!(
                "Target directory '{}' already exists and is not empty",
                target_dir.display()
            ));
        }

        std::fs::remove_dir_all(target_dir).with_context(|| {
            format!(
                "Failed to remove existing empty target directory: {}",
                target_dir.display()
            )
        })?;
    }

    std::fs::create_dir_all(&temp_dir).with_context(|| {
        format!(
            "Failed to create temporary download directory: {}",
            temp_dir.display()
        )
    })?;

    let download_result = storage
        .download_backup_verified(backup_id, &temp_dir, verify_checksums)
        .await
        .map_err(|e| anyhow!("Failed to download backup: {}", e));

    let mut result = match download_result {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(e);
        }
    };

    // Check checksum verification result (downloaded into temp dir)
    if let Some(ref checksum_result) = result.checksum_verified {
        if !checksum_result.all_matched {
            error!(
                "[backups-download] Checksum verification failed: {} mismatches",
                checksum_result.files_mismatched
            );
            for mismatch in &checksum_result.mismatches {
                error!(
                    "  - {}: expected {}, got {}",
                    mismatch.file, mismatch.expected, mismatch.actual
                );
            }
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(anyhow!(
                "Checksum verification failed: {} files have mismatched checksums",
                checksum_result.files_mismatched
            ));
        }
    }

    std::fs::rename(&temp_dir, target_dir).with_context(|| {
        format!(
            "Failed to move verified backup from '{}' to '{}'",
            temp_dir.display(),
            target_dir.display()
        )
    })?;

    result.target_dir = target_dir.display().to_string();

    info!(
        "[backups-download] Download complete: {} files, {} bytes in {}s",
        result.files_downloaded, result.bytes_downloaded, result.duration_secs
    );

    Ok(result)
}

/// Format list results as a table
pub fn format_list_table(result: &ListBackupsResult) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&format!(
        "{:<38} {:<10} {:<10} {:<20} {:>12} {:<10}\n",
        "BACKUP ID", "TYPE", "STATUS", "TIMESTAMP", "SIZE", "PG VERSION"
    ));
    output.push_str(&"-".repeat(110));
    output.push('\n');

    // Rows
    for backup in &result.backups {
        let type_str = format_backup_type(&backup.backup_type);
        let status_str = format_backup_status(&backup.status);
        let timestamp = backup.start_time.format("%Y-%m-%d %H:%M:%S").to_string();
        let size_str = format_size(backup.size_bytes);
        let pinned_marker = if backup.pinned { " 📌" } else { "" };

        output.push_str(&format!(
            "{:<38} {:<10} {:<10} {:<20} {:>12} {:<10}{}\n",
            backup.id,
            type_str,
            status_str,
            timestamp,
            size_str,
            backup.server_version,
            pinned_marker
        ));
    }

    // Summary
    output.push_str(&"-".repeat(110));
    output.push('\n');
    output.push_str(&format!(
        "Total: {} backups, {} total size\n",
        result.total_count,
        format_size(result.total_size_bytes)
    ));

    output
}

/// Format list results as JSON
pub fn format_list_json(result: &ListBackupsResult) -> Result<String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "backups": result.backups,
        "total_count": result.total_count,
        "total_size_bytes": result.total_size_bytes,
        "total_size_human": format_size(result.total_size_bytes),
    }))
    .map_err(|e| anyhow!("Failed to serialize to JSON: {}", e))
}

/// Format show results as a table
pub fn format_show_table(result: &ShowBackupResult) -> String {
    let mut output = String::new();
    let details = &result.details;
    let metadata = &details.metadata;

    output.push_str("=== Backup Details ===\n\n");

    // Basic info
    output.push_str(&format!("Backup ID:       {}\n", metadata.id));
    output.push_str(&format!(
        "Type:            {}\n",
        format_backup_type(&metadata.backup_type)
    ));
    output.push_str(&format!(
        "Status:          {}\n",
        format_backup_status(&metadata.status)
    ));
    output.push_str(&format!(
        "Pinned:          {}\n",
        if metadata.pinned { "Yes" } else { "No" }
    ));
    output.push('\n');

    // Timestamps
    output.push_str("=== Timestamps ===\n\n");
    output.push_str(&format!(
        "Start Time:      {}\n",
        metadata.start_time.format("%Y-%m-%d %H:%M:%S UTC")
    ));
    if let Some(end_time) = metadata.end_time {
        output.push_str(&format!(
            "End Time:        {}\n",
            end_time.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        let duration = end_time - metadata.start_time;
        output.push_str(&format!("Duration:        {}s\n", duration.num_seconds()));
    }
    output.push('\n');

    // Size info
    output.push_str("=== Size Information ===\n\n");
    output.push_str(&format!(
        "Metadata Size:   {}\n",
        format_size(metadata.size_bytes)
    ));
    output.push_str(&format!(
        "Storage Size:    {}\n",
        format_size(details.total_storage_size)
    ));
    output.push_str(&format!("File Count:      {}\n", metadata.files.len()));
    output.push_str(&format!("Object Count:    {}\n", details.objects.len()));
    output.push('\n');

    // Storage location
    output.push_str("=== Storage Location ===\n\n");
    output.push_str(&format!("Bucket:          {}\n", details.bucket));
    output.push_str(&format!("Key Prefix:      {}\n", details.storage_key));
    output.push('\n');

    // PostgreSQL info
    output.push_str("=== PostgreSQL Info ===\n\n");
    output.push_str(&format!("Server Version:  {}\n", metadata.server_version));
    if let Some(ref wal_start) = metadata.wal_start {
        output.push_str(&format!("WAL Start:       {}\n", wal_start));
    }
    if let Some(ref wal_end) = metadata.wal_end {
        output.push_str(&format!("WAL End:         {}\n", wal_end));
    }
    if let Some(ref base_id) = metadata.base_backup_id {
        output.push_str(&format!("Base Backup ID:  {}\n", base_id));
    }
    output.push('\n');

    // Checksum
    if let Some(ref checksum) = metadata.checksum {
        output.push_str("=== Integrity ===\n\n");
        output.push_str(&format!("Checksum (SHA256): {}\n", checksum));
        output.push('\n');
    }

    // Tags
    if !metadata.tags.is_empty() {
        output.push_str("=== Tags/Labels ===\n\n");
        for tag in &metadata.tags {
            output.push_str(&format!("  - {}\n", tag));
        }
        output.push('\n');
    }

    // Files (truncated if too many)
    output.push_str("=== Files ===\n\n");
    let max_files = 20;
    for (i, file) in metadata.files.iter().enumerate() {
        if i >= max_files {
            output.push_str(&format!(
                "  ... and {} more files\n",
                metadata.files.len() - max_files
            ));
            break;
        }
        let checksum_str = file
            .checksum
            .as_ref()
            .map(|c| format!(" [{}]", &c[..8]))
            .unwrap_or_default();
        output.push_str(&format!(
            "  - {} ({}){}\n",
            file.name,
            format_size(file.size),
            checksum_str
        ));
    }

    output
}

/// Format show results as JSON
pub fn format_show_json(result: &ShowBackupResult) -> Result<String> {
    serde_json::to_string_pretty(&result.details)
        .map_err(|e| anyhow!("Failed to serialize to JSON: {}", e))
}

/// Format download results as a table
pub fn format_download_table(result: &DownloadResult) -> String {
    let mut output = String::new();

    output.push_str("=== Download Complete ===\n\n");
    output.push_str(&format!("Backup ID:       {}\n", result.backup_id));
    output.push_str(&format!("Target Dir:      {}\n", result.target_dir));
    output.push_str(&format!("Files:           {}\n", result.files_downloaded));
    output.push_str(&format!(
        "Size:            {}\n",
        format_size(result.bytes_downloaded)
    ));
    output.push_str(&format!("Duration:        {}s\n", result.duration_secs));

    if let Some(ref checksum) = result.checksum_verified {
        output.push('\n');
        output.push_str("=== Checksum Verification ===\n\n");
        output.push_str(&format!(
            "Status:          {}\n",
            if checksum.all_matched {
                "✓ PASSED"
            } else {
                "✗ FAILED"
            }
        ));
        output.push_str(&format!("Files Verified:  {}\n", checksum.files_verified));
        output.push_str(&format!("Files Matched:   {}\n", checksum.files_matched));
        output.push_str(&format!(
            "Files Mismatched: {}\n",
            checksum.files_mismatched
        ));
        output.push_str(&format!("Files Skipped:   {}\n", checksum.files_skipped));

        if !checksum.mismatches.is_empty() {
            output.push('\n');
            output.push_str("Mismatched Files:\n");
            for mismatch in &checksum.mismatches {
                output.push_str(&format!("  - {}\n", mismatch.file));
                output.push_str(&format!("    Expected: {}\n", mismatch.expected));
                output.push_str(&format!("    Actual:   {}\n", mismatch.actual));
            }
        }
    }

    output
}

/// Format download results as JSON
pub fn format_download_json(result: &DownloadResult) -> Result<String> {
    serde_json::to_string_pretty(result).map_err(|e| anyhow!("Failed to serialize to JSON: {}", e))
}

// Helper functions

async fn create_storage_provider(opts: &BackupStorageOptions) -> Result<PostgresBackupStorage> {
    let bucket = opts
        .bucket
        .clone()
        .ok_or_else(|| anyhow!("Storage bucket name is required (--storage-bucket)"))?;

    let provider_type = match opts.provider_type.as_deref() {
        Some("s3") | None => StorageProviderType::S3,
        Some(other) => return Err(anyhow!("Unsupported storage provider type: {}", other)),
    };

    PostgresBackupStorage::new(
        provider_type,
        bucket,
        opts.prefix.clone(),
        opts.region.clone(),
        opts.endpoint.clone(),
        opts.access_key.clone(),
        opts.secret_key.clone(),
        None, // account_id (optional; unused by current S3/MinIO provider)
        None, // project_id (optional; unused by current S3/MinIO provider)
        None, // credentials_path (optional; unused by current S3/MinIO provider)
    )
    .await
    .map_err(|e| anyhow!("Failed to create storage provider: {}", e))
}

fn parse_backup_type(s: &str) -> Result<StorageBackupType> {
    match s.to_lowercase().as_str() {
        "full" => Ok(StorageBackupType::Full),
        "incremental" => Ok(StorageBackupType::Incremental),
        "snapshot" => Ok(StorageBackupType::Snapshot),
        _ => Err(anyhow!(
            "Invalid backup type '{}'. Valid values: full, incremental, snapshot",
            s
        )),
    }
}

fn parse_backup_status(s: &str) -> Result<StorageBackupStatus> {
    match s.to_lowercase().as_str() {
        "completed" | "complete" => Ok(StorageBackupStatus::Completed),
        "in_progress" | "inprogress" | "running" => Ok(StorageBackupStatus::InProgress),
        "failed" | "error" => Ok(StorageBackupStatus::Failed),
        _ => Err(anyhow!(
            "Invalid backup status '{}'. Valid values: completed, in_progress, failed",
            s
        )),
    }
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    // Try RFC3339 first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }

    let datetime_formats = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"];
    for fmt in &datetime_formats {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(DateTime::from_naive_utc_and_offset(dt, Utc));
        }
    }

    let date_formats = ["%Y-%m-%d"];
    for fmt in &date_formats {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(s, fmt) {
            let dt = date.and_hms_opt(0, 0, 0).unwrap();
            return Ok(DateTime::from_naive_utc_and_offset(dt, Utc));
        }
    }

    Err(anyhow!(
        "Invalid datetime format '{}'. Use RFC3339 (e.g., 2025-01-15T10:30:00Z) or YYYY-MM-DD",
        s
    ))
}

fn format_backup_type(bt: &StorageBackupType) -> &'static str {
    match bt {
        StorageBackupType::Full => "Full",
        StorageBackupType::Incremental => "Incremental",
        StorageBackupType::Snapshot => "Snapshot",
    }
}

fn format_backup_status(status: &StorageBackupStatus) -> &'static str {
    match status {
        StorageBackupStatus::Completed => "Completed",
        StorageBackupStatus::InProgress => "InProgress",
        StorageBackupStatus::Failed => "Failed",
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_backup_type() {
        assert!(matches!(
            parse_backup_type("full").unwrap(),
            StorageBackupType::Full
        ));
        assert!(matches!(
            parse_backup_type("FULL").unwrap(),
            StorageBackupType::Full
        ));
        assert!(matches!(
            parse_backup_type("snapshot").unwrap(),
            StorageBackupType::Snapshot
        ));
        assert!(matches!(
            parse_backup_type("incremental").unwrap(),
            StorageBackupType::Incremental
        ));
        assert!(parse_backup_type("invalid").is_err());
    }

    #[test]
    fn test_parse_backup_status() {
        assert!(matches!(
            parse_backup_status("completed").unwrap(),
            StorageBackupStatus::Completed
        ));
        assert!(matches!(
            parse_backup_status("COMPLETE").unwrap(),
            StorageBackupStatus::Completed
        ));
        assert!(matches!(
            parse_backup_status("in_progress").unwrap(),
            StorageBackupStatus::InProgress
        ));
        assert!(matches!(
            parse_backup_status("failed").unwrap(),
            StorageBackupStatus::Failed
        ));
        assert!(parse_backup_status("invalid").is_err());
    }

    #[test]
    fn test_parse_datetime() {
        // RFC3339
        assert!(parse_datetime("2025-01-15T10:30:00Z").is_ok());
        assert!(parse_datetime("2025-01-15T10:30:00+00:00").is_ok());

        // Date only
        assert!(parse_datetime("2025-01-15").is_ok());

        // Invalid
        assert!(parse_datetime("not-a-date").is_err());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 1024), "1.00 TB");
    }
}
