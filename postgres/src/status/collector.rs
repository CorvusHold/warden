//! Status collector for gathering backup, PITR, and retention status.

use chrono::{Duration, Utc};
use log::{debug, info, warn};
use std::path::PathBuf;

use storage::{BackupMetadata, BackupStatus as StorageBackupStatus, PostgresBackupStorage, StorageProviderType};

use crate::common::BackupCatalog;
use crate::pitr::PitrPlanner;
use crate::retention::policy::PitrRetentionPolicy;
use crate::PostgresError;

use super::types::*;

/// Configuration for the status collector.
#[derive(Debug, Clone)]
pub struct StatusCollectorConfig {
    /// Local backup directory
    pub backup_dir: PathBuf,
    /// WAL archive directory (optional)
    pub wal_archive_dir: Option<PathBuf>,
    /// Remote storage configuration (optional)
    pub storage_config: Option<StorageConfig>,
    /// Retention policy file path (optional)
    pub retention_policy_path: Option<PathBuf>,
    /// Database name for context
    pub database: Option<String>,
    /// Host for context
    pub host: Option<String>,
    /// Thresholds for health checks
    pub thresholds: StatusThresholds,
}

/// Storage configuration for remote storage.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub bucket: String,
    pub prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

/// Thresholds for determining health status.
#[derive(Debug, Clone)]
pub struct StatusThresholds {
    /// Maximum age of last backup before warning (hours)
    pub backup_warning_age_hours: u32,
    /// Maximum age of last backup before critical (hours)
    pub backup_critical_age_hours: u32,
    /// Minimum PITR window before warning (hours)
    pub pitr_warning_window_hours: u32,
    /// Minimum PITR window before critical (hours)
    pub pitr_critical_window_hours: u32,
    /// Minimum number of backups before warning
    pub min_backups_warning: usize,
}

impl Default for StatusThresholds {
    fn default() -> Self {
        Self {
            backup_warning_age_hours: 24,
            backup_critical_age_hours: 48,
            pitr_warning_window_hours: 12,
            pitr_critical_window_hours: 4,
            min_backups_warning: 2,
        }
    }
}

/// Collects status information from various sources.
pub struct StatusCollector {
    config: StatusCollectorConfig,
}

impl StatusCollector {
    /// Create a new status collector.
    pub fn new(config: StatusCollectorConfig) -> Self {
        Self { config }
    }

    /// Create a simple collector with just a backup directory.
    pub fn with_backup_dir(backup_dir: PathBuf) -> Self {
        Self::new(StatusCollectorConfig {
            backup_dir,
            wal_archive_dir: None,
            storage_config: None,
            retention_policy_path: None,
            database: None,
            host: None,
            thresholds: StatusThresholds::default(),
        })
    }

    /// Set remote storage configuration.
    pub fn with_storage_config(mut self, config: StorageConfig) -> Self {
        self.config.storage_config = Some(config);
        self
    }

    /// Create storage provider from config.
    async fn create_storage(&self) -> Result<Option<PostgresBackupStorage>, PostgresError> {
        match &self.config.storage_config {
            Some(cfg) => {
                let storage = PostgresBackupStorage::new(
                    StorageProviderType::S3,
                    cfg.bucket.clone(),
                    cfg.prefix.clone(),
                    cfg.region.clone(),
                    cfg.endpoint.clone(),
                    cfg.access_key.clone(),
                    cfg.secret_key.clone(),
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| PostgresError::BackupError(format!("Failed to create storage: {}", e)))?;
                Ok(Some(storage))
            }
            None => Ok(None),
        }
    }

    /// Set WAL archive directory.
    pub fn with_wal_archive_dir(mut self, dir: PathBuf) -> Self {
        self.config.wal_archive_dir = Some(dir);
        self
    }

    /// Set retention policy path.
    pub fn with_retention_policy(mut self, path: PathBuf) -> Self {
        self.config.retention_policy_path = Some(path);
        self
    }

    /// Set database context.
    pub fn with_database(mut self, database: String) -> Self {
        self.config.database = Some(database);
        self
    }

    /// Set host context.
    pub fn with_host(mut self, host: String) -> Self {
        self.config.host = Some(host);
        self
    }

    /// Collect overall status.
    pub async fn collect_status(&self) -> Result<OverallStatus, PostgresError> {
        info!("Collecting status information...");

        let backup_status = self.collect_backup_status().await?;
        let pitr_status = self.collect_pitr_status().await?;
        let retention_status = self.collect_retention_status().await?;
        let storage_status = self.collect_storage_status().await?;

        let mut issues = Vec::new();

        // Collect issues from all components
        for issue in &backup_status.issues {
            issues.push(StatusIssue {
                severity: backup_status.health,
                category: "backup".to_string(),
                message: issue.clone(),
                suggestion: None,
            });
        }

        for issue in &pitr_status.issues {
            issues.push(StatusIssue {
                severity: pitr_status.health,
                category: "pitr".to_string(),
                message: issue.clone(),
                suggestion: None,
            });
        }

        for issue in &retention_status.issues {
            issues.push(StatusIssue {
                severity: retention_status.health,
                category: "retention".to_string(),
                message: issue.clone(),
                suggestion: None,
            });
        }

        let mut status = OverallStatus {
            collected_at: Utc::now(),
            health: HealthStatus::Unknown,
            backup: backup_status,
            pitr: pitr_status,
            retention: retention_status,
            schedules: None, // Will be populated if schedule config is available
            storage: storage_status,
            issues,
        };

        status.compute_health();

        info!("Status collection complete: health={}", status.health);
        Ok(status)
    }

    /// Collect backup status.
    pub async fn collect_backup_status(&self) -> Result<BackupStatus, PostgresError> {
        debug!("Collecting backup status...");

        let mut status = BackupStatus::default();
        let mut backups: Vec<BackupInfo> = Vec::new();

        // Load local backup catalog
        let catalog_path = self.config.backup_dir.join("backup_catalog.json");
        if catalog_path.exists() {
            match BackupCatalog::load_from_file(&catalog_path) {
                Ok(catalog) => {
                    for backup in catalog.backups {
                        backups.push(BackupInfo {
                            id: backup.id.to_string(),
                            backup_type: format!("{:?}", backup.backup_type),
                            start_time: backup.start_time,
                            end_time: backup.end_time,
                            size_bytes: backup.size_bytes.unwrap_or(0),
                            success: backup.status == crate::common::BackupStatus::Completed,
                            error: backup.error_message,
                            location: Some(backup.backup_path.to_string_lossy().to_string()),
                            database: self.config.database.clone(),
                            encrypted: false, // TODO: Detect from backup metadata
                            encryption_algorithm: None,
                        });
                    }
                }
                Err(e) => {
                    warn!("Failed to load backup catalog: {}", e);
                }
            }
        }

        // Scan backup directories for metadata files
        if let Ok(entries) = std::fs::read_dir(&self.config.backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let metadata_path = path.join("backup_metadata.json");
                if metadata_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&metadata_path) {
                        if let Ok(metadata) = serde_json::from_str::<BackupMetadata>(&content) {
                            // Skip if already in list
                            if backups.iter().any(|b| b.id == metadata.id) {
                                continue;
                            }

                            backups.push(BackupInfo {
                                id: metadata.id.clone(),
                                backup_type: format!("{:?}", metadata.backup_type),
                                start_time: metadata.start_time,
                                end_time: metadata.end_time,
                                size_bytes: metadata.size_bytes,
                                success: metadata.status == StorageBackupStatus::Completed,
                                error: None,
                                location: Some(path.to_string_lossy().to_string()),
                                database: self.config.database.clone(),
                                encrypted: metadata.encrypted.unwrap_or(false),
                                encryption_algorithm: metadata.encryption_algorithm.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Load remote backups if storage is configured
        if let Some(storage) = self.create_storage().await? {
            match storage.list_remote_backups_detailed().await {
                Ok(remote_backups) => {
                    for metadata in remote_backups {
                        // Skip if already in list
                        if backups.iter().any(|b| b.id == metadata.id) {
                            continue;
                        }

                        backups.push(BackupInfo {
                            id: metadata.id.clone(),
                            backup_type: format!("{:?}", metadata.backup_type),
                            start_time: metadata.start_time,
                            end_time: metadata.end_time,
                            size_bytes: metadata.size_bytes,
                            success: metadata.status == StorageBackupStatus::Completed,
                            error: None,
                            location: Some(format!("remote:{}", metadata.id)),
                            database: self.config.database.clone(),
                            encrypted: metadata.encrypted.unwrap_or(false),
                            encryption_algorithm: metadata.encryption_algorithm.clone(),
                        });
                    }
                }
                Err(e) => {
                    warn!("Failed to list remote backups: {}", e);
                }
            }
        }

        // Sort by start time (newest first)
        backups.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        // Calculate statistics
        status.total_backups = backups.len();
        status.successful_backups = backups.iter().filter(|b| b.success).count();
        status.failed_backups = backups.iter().filter(|b| !b.success).count();
        status.encrypted_backups = backups.iter().filter(|b| b.encrypted).count();
        status.unencrypted_backups = backups.iter().filter(|b| !b.encrypted).count();

        // Find last successful and last attempt
        status.last_successful = backups.iter().find(|b| b.success).cloned();
        status.last_attempt = backups.first().cloned();

        // Calculate last backup age
        if let Some(ref last) = status.last_successful {
            let age = Utc::now() - last.start_time;
            status.last_backup_age = Some(age);
        }

        // Calculate average interval between successful backups
        let successful: Vec<_> = backups.iter().filter(|b| b.success).collect();
        if successful.len() >= 2 {
            let mut intervals: Vec<Duration> = Vec::new();
            for i in 0..successful.len() - 1 {
                let interval = successful[i].start_time - successful[i + 1].start_time;
                intervals.push(interval);
            }
            let total_secs: i64 = intervals.iter().map(|d| d.num_seconds()).sum();
            let avg_secs = total_secs / intervals.len() as i64;
            status.average_interval = Some(Duration::seconds(avg_secs));
        }

        // Determine health status
        status.health = self.evaluate_backup_health(&status);

        debug!(
            "Backup status: {} total, {} successful, health={}",
            status.total_backups, status.successful_backups, status.health
        );

        Ok(status)
    }

    /// Evaluate backup health based on thresholds.
    fn evaluate_backup_health(&self, status: &BackupStatus) -> HealthStatus {
        let mut health = HealthStatus::Healthy;
        let thresholds = &self.config.thresholds;

        // Check if we have any backups
        if status.total_backups == 0 {
            return HealthStatus::Critical;
        }

        // Check backup age
        if let Some(age) = status.last_backup_age {
            let age_hours = age.num_hours() as u32;
            if age_hours >= thresholds.backup_critical_age_hours {
                health = HealthStatus::Critical;
            } else if age_hours >= thresholds.backup_warning_age_hours {
                health = HealthStatus::Warning;
            }
        } else {
            // No successful backups
            health = HealthStatus::Critical;
        }

        // Check minimum backup count
        if status.successful_backups < thresholds.min_backups_warning
            && health != HealthStatus::Critical {
                health = HealthStatus::Warning;
            }

        // Check for recent failures
        if let Some(ref last_attempt) = status.last_attempt {
            if !last_attempt.success
                && health != HealthStatus::Critical {
                    health = HealthStatus::Warning;
                }
        }

        health
    }

    /// Collect PITR status.
    pub async fn collect_pitr_status(&self) -> Result<PitrStatus, PostgresError> {
        debug!("Collecting PITR status...");

        let mut status = PitrStatus::default();

        // Create a PITR planner to discover recovery options
        let mut planner = PitrPlanner::new(self.config.backup_dir.clone());

        if let Some(wal_dir) = &self.config.wal_archive_dir {
            planner = planner.with_wal_archive_dir(wal_dir.clone());
        }

        if let Some(storage) = self.create_storage().await? {
            planner = planner.with_storage(storage);
        }

        match planner.list_recovery_options().await {
            Ok(options) => {
                status.available = !options.available_backups.is_empty();
                status.base_backup_count = options.available_backups.len();
                status.earliest_recovery_point = options.earliest_recoverable;
                status.latest_recovery_point = options.latest_recoverable;

                // Calculate recovery window
                if let (Some(earliest), Some(latest)) =
                    (options.earliest_recoverable, options.latest_recoverable)
                {
                    status.recovery_window = Some(latest - earliest);
                }

                // WAL coverage info
                status.wal_segment_count = options.wal_coverage.segment_count;
                status.wal_size_bytes = options.wal_coverage.total_size_bytes;

                // Check for gaps
                for gap in &options.wal_coverage.gaps {
                    status.wal_gaps.push(WalGap {
                        start: gap.start_segment.clone(),
                        end: gap.end_segment.clone(),
                        time_range: None,
                    });
                }
            }
            Err(e) => {
                warn!("Failed to collect PITR options: {}", e);
                status.issues.push(format!("Failed to analyze PITR: {}", e));
            }
        }

        // Determine health status
        status.health = self.evaluate_pitr_health(&status);

        debug!(
            "PITR status: available={}, window={:?}, health={}",
            status.available, status.recovery_window, status.health
        );

        Ok(status)
    }

    /// Evaluate PITR health based on thresholds.
    fn evaluate_pitr_health(&self, status: &PitrStatus) -> HealthStatus {
        let thresholds = &self.config.thresholds;

        if !status.available {
            return HealthStatus::Critical;
        }

        if status.base_backup_count == 0 {
            return HealthStatus::Critical;
        }

        // Check recovery window size
        if let Some(window) = status.recovery_window {
            let window_hours = window.num_hours() as u32;
            if window_hours < thresholds.pitr_critical_window_hours {
                return HealthStatus::Critical;
            } else if window_hours < thresholds.pitr_warning_window_hours {
                return HealthStatus::Warning;
            }
        }

        // Check for WAL gaps
        if !status.wal_gaps.is_empty() {
            return HealthStatus::Warning;
        }

        HealthStatus::Healthy
    }

    /// Collect retention status.
    pub async fn collect_retention_status(&self) -> Result<RetentionStatus, PostgresError> {
        debug!("Collecting retention status...");

        let mut status = RetentionStatus::default();

        // Try to load retention policy
        if let Some(policy_path) = &self.config.retention_policy_path {
            if policy_path.exists() {
                match std::fs::read_to_string(policy_path) {
                    Ok(content) => match serde_json::from_str::<PitrRetentionPolicy>(&content) {
                        Ok(policy) => {
                            status.policy_configured = true;
                            status.policy_name = Some(format!("v{}", policy.version));
                            status.pitr_window_hours =
                                Some(policy.wal_retention.pitr_window_hours);
                            status.min_backups_to_keep =
                                Some(policy.safety.min_successful_backups);
                            status.health = HealthStatus::Healthy;
                        }
                        Err(e) => {
                            status.issues.push(format!("Invalid retention policy: {}", e));
                            status.health = HealthStatus::Warning;
                        }
                    },
                    Err(e) => {
                        status.issues.push(format!("Failed to read retention policy: {}", e));
                        status.health = HealthStatus::Warning;
                    }
                }
            }
        }

        // If no policy configured, set to unknown but not critical
        if !status.policy_configured {
            status.health = HealthStatus::Unknown;
        }

        debug!(
            "Retention status: configured={}, health={}",
            status.policy_configured, status.health
        );

        Ok(status)
    }

    /// Collect storage status.
    pub async fn collect_storage_status(&self) -> Result<StorageStatus, PostgresError> {
        debug!("Collecting storage status...");

        let mut status = StorageStatus::default();

        // Calculate local storage usage
        if self.config.backup_dir.exists() {
            let mut backup_size = 0u64;
            let mut backup_count = 0usize;
            let mut wal_size = 0u64;
            let mut wal_count = 0usize;

            if let Ok(entries) = std::fs::read_dir(&self.config.backup_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let size = calculate_dir_size(&path);
                        if path
                            .file_name()
                            .map(|n| n.to_string_lossy().contains("wal"))
                            .unwrap_or(false)
                        {
                            wal_size += size;
                            wal_count += 1;
                        } else {
                            backup_size += size;
                            backup_count += 1;
                        }
                    }
                }
            }

            status.local = Some(StorageUsage {
                used_bytes: backup_size + wal_size,
                backup_count,
                wal_count,
                backup_size_bytes: backup_size,
                wal_size_bytes: wal_size,
                location: self.config.backup_dir.to_string_lossy().to_string(),
            });
        }

        // Calculate remote storage usage if configured
        if let Some(storage) = self.create_storage().await? {
            let bucket_name = self.config.storage_config.as_ref().map(|c| c.bucket.clone()).unwrap_or_default();
            match storage.list_all_objects().await {
                Ok(objects) => {
                    let mut backup_size = 0u64;
                    let mut backup_count = 0usize;
                    let mut wal_size = 0u64;
                    let mut wal_count = 0usize;

                    for obj in objects {
                        let size = obj.size.unwrap_or(0);
                        if obj.key.contains("wal") || obj.key.contains("pg_wal") {
                            wal_size += size;
                            wal_count += 1;
                        } else {
                            backup_size += size;
                            backup_count += 1;
                        }
                    }

                    status.remote = Some(StorageUsage {
                        used_bytes: backup_size + wal_size,
                        backup_count,
                        wal_count,
                        backup_size_bytes: backup_size,
                        wal_size_bytes: wal_size,
                        location: format!("s3://{}", bucket_name),
                    });
                }
                Err(e) => {
                    warn!("Failed to list remote storage: {}", e);
                    status.issues.push(format!("Failed to access remote storage: {}", e));
                }
            }
        }

        status.health = HealthStatus::Healthy;

        debug!("Storage status collected");

        Ok(status)
    }

    /// Collect metric gauges for export.
    pub async fn collect_metrics(&self) -> Result<MetricGauges, PostgresError> {
        let status = self.collect_status().await?;

        let mut gauges = MetricGauges::default();

        // Backup age
        if let Some(age) = status.backup.last_backup_age {
            gauges.latest_backup_age_seconds = Some(age.num_seconds() as f64);
        }

        // PITR window
        if let Some(window) = status.pitr.recovery_window {
            gauges.pitr_window_seconds = Some(window.num_seconds() as f64);
        }

        // Backup counts
        gauges.available_backups = status.backup.successful_backups as u64;
        gauges.wal_segments = status.pitr.wal_segment_count as u64;

        // Storage
        if let Some(local) = status.storage.local {
            gauges.backup_storage_bytes += local.backup_size_bytes;
            gauges.wal_storage_bytes += local.wal_size_bytes;
        }
        if let Some(remote) = status.storage.remote {
            gauges.backup_storage_bytes += remote.backup_size_bytes;
            gauges.wal_storage_bytes += remote.wal_size_bytes;
        }

        Ok(gauges)
    }
}

/// Calculate total size of a directory.
fn calculate_dir_size(path: &std::path::Path) -> u64 {
    let mut size = 0u64;
    for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Ok(metadata) = entry.metadata() {
                size += metadata.len();
            }
        }
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_status_collector_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let collector = StatusCollector::with_backup_dir(temp_dir.path().to_path_buf());

        let status = collector.collect_status().await.unwrap();
        assert_eq!(status.backup.total_backups, 0);
        assert_eq!(status.backup.health, HealthStatus::Critical);
    }

    #[test]
    fn test_calculate_dir_size() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world").unwrap();

        let size = calculate_dir_size(temp_dir.path());
        assert_eq!(size, 11); // "hello world" is 11 bytes
    }
}
