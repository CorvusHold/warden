//! PITR recovery plan computation.

use chrono::{DateTime, Utc};
use log::{debug, info, warn};
use std::path::PathBuf;
use uuid::Uuid;

use storage::{
    BackupMetadata, BackupStatus, BackupType, PostgresBackupStorage, StorageProviderType,
};

use crate::common::BackupCatalog;
use crate::PostgresError;

use super::types::{BaseBackupInfo, PlanValidation, RecoveryPlan, RecoveryTarget, RecoveryWindow};
use super::wal::{RemoteWalObject, WalInventory};

/// PITR planner for computing recovery plans.
pub struct PitrPlanner {
    /// Local backup directory.
    backup_dir: PathBuf,
    /// Optional remote storage.
    storage: Option<PostgresBackupStorage>,
    /// WAL archive directory (local).
    wal_archive_dir: Option<PathBuf>,
    /// WAL prefix in remote storage.
    wal_prefix: Option<String>,
}

impl PitrPlanner {
    /// Create a new PITR planner with local backup directory.
    pub fn new(backup_dir: PathBuf) -> Self {
        Self {
            backup_dir,
            storage: None,
            wal_archive_dir: None,
            wal_prefix: None,
        }
    }

    /// Set remote storage for the planner.
    pub fn with_storage(mut self, storage: PostgresBackupStorage) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Set local WAL archive directory.
    pub fn with_wal_archive_dir(mut self, dir: PathBuf) -> Self {
        self.wal_archive_dir = Some(dir);
        self
    }

    /// Set WAL prefix in remote storage.
    pub fn with_wal_prefix(mut self, prefix: String) -> Self {
        self.wal_prefix = Some(prefix);
        self
    }

    /// Create a planner with remote storage configuration.
    #[allow(clippy::too_many_arguments)]
    pub async fn with_remote_storage(
        backup_dir: PathBuf,
        provider_type: StorageProviderType,
        bucket: String,
        prefix: Option<String>,
        region: Option<String>,
        endpoint: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
    ) -> Result<Self, PostgresError> {
        let storage = PostgresBackupStorage::new(
            provider_type,
            bucket,
            prefix,
            region,
            endpoint,
            access_key,
            secret_key,
            None,
            None,
            None,
        )
        .await
        .map_err(|e| PostgresError::BackupError(format!("Failed to create storage: {}", e)))?;

        Ok(Self {
            backup_dir,
            storage: Some(storage),
            wal_archive_dir: None,
            wal_prefix: None,
        })
    }

    /// Plan a recovery to the specified target.
    pub async fn plan_recovery(
        &self,
        target: RecoveryTarget,
    ) -> Result<RecoveryPlan, PostgresError> {
        info!("Planning PITR recovery to target: {:?}", target);

        // Step 1: Find available base backups
        let base_backups = self.discover_base_backups().await?;
        if base_backups.is_empty() {
            return Err(PostgresError::BackupError(
                "No base backups found for PITR".to_string(),
            ));
        }

        // Step 2: Select the best base backup for the target
        let base_backup = self.select_base_backup(&base_backups, &target)?;
        info!(
            "Selected base backup: {} ({})",
            base_backup.id, base_backup.start_time
        );

        // Step 3: Discover WAL segments
        let mut wal_inventory = WalInventory::new();
        self.discover_wal_segments(&mut wal_inventory).await?;

        let wal_coverage = wal_inventory.calculate_coverage();
        info!(
            "WAL coverage: {} segments, {:?} to {:?}",
            wal_coverage.segment_count, wal_coverage.earliest_time, wal_coverage.latest_time
        );

        // Step 4: Validate target is reachable
        let validation = self.validate_target(&target, &base_backup, &wal_inventory)?;
        if !validation.is_valid {
            return Err(PostgresError::RestoreError(format!(
                "PITR target validation failed: {}",
                validation.errors.join("; ")
            )));
        }

        // Step 5: Get required WAL segments
        let wal_start = base_backup.wal_start.as_deref().unwrap_or("0/0");
        let target_lsn = match &target {
            RecoveryTarget::Lsn(lsn) => Some(lsn.as_str()),
            _ => None,
        };
        let target_time = target.as_time();

        let wal_segments =
            wal_inventory.get_segments_for_recovery(wal_start, target_lsn, target_time)?;

        info!("Recovery requires {} WAL segments", wal_segments.len());

        // Step 6: Calculate recovery window
        let recovery_window = RecoveryWindow {
            earliest: base_backup.start_time,
            latest: wal_coverage.latest_time,
            target_in_window: validation.is_valid,
        };

        // Step 7: Calculate estimated download size
        let estimated_download_bytes = if base_backup.is_remote {
            base_backup.size_bytes
        } else {
            0
        } + wal_segments
            .iter()
            .filter(|s| s.is_remote)
            .map(|s| s.size_bytes)
            .sum::<u64>();

        let plan = RecoveryPlan {
            id: Uuid::new_v4(),
            computed_at: Utc::now(),
            target,
            base_backup,
            wal_segments,
            recovery_window,
            validation,
            estimated_download_bytes,
        };

        info!("Recovery plan computed: {}", plan.id);
        Ok(plan)
    }

    /// Discover available base backups from local and remote sources.
    async fn discover_base_backups(&self) -> Result<Vec<BaseBackupInfo>, PostgresError> {
        let mut backups = Vec::new();

        // Discover local backups
        let local_backups = self.discover_local_backups()?;
        backups.extend(local_backups);

        // Discover remote backups
        if let Some(storage) = &self.storage {
            let remote_backups = self.discover_remote_backups(storage).await?;
            backups.extend(remote_backups);
        }

        // Sort by start time (newest first)
        backups.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        info!("Discovered {} base backups", backups.len());
        Ok(backups)
    }

    /// Discover local backups from the backup directory.
    fn discover_local_backups(&self) -> Result<Vec<BaseBackupInfo>, PostgresError> {
        let mut backups = Vec::new();

        // Try to load the backup catalog
        let catalog_path = self.backup_dir.join("backup_catalog.json");
        if catalog_path.exists() {
            match BackupCatalog::load_from_file(&catalog_path) {
                Ok(catalog) => {
                    for backup in catalog.backups {
                        // Only include completed full or snapshot backups
                        if backup.status != crate::common::BackupStatus::Completed {
                            continue;
                        }
                        if backup.backup_type != crate::common::BackupType::Full
                            && backup.backup_type != crate::common::BackupType::Snapshot
                        {
                            continue;
                        }

                        backups.push(BaseBackupInfo {
                            id: backup.id,
                            path: backup.backup_path.to_string_lossy().to_string(),
                            start_time: backup.start_time,
                            end_time: backup.end_time,
                            wal_start: backup.wal_start,
                            wal_end: backup.wal_end,
                            server_version: backup.server_version,
                            size_bytes: backup.size_bytes.unwrap_or(0),
                            is_remote: false,
                        });
                    }
                }
                Err(e) => {
                    warn!("Failed to load backup catalog: {}", e);
                }
            }
        }

        // Also scan for backup directories with metadata files
        if let Ok(entries) = std::fs::read_dir(&self.backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let metadata_path = path.join("backup_metadata.json");
                if metadata_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&metadata_path) {
                        if let Ok(metadata) = serde_json::from_str::<BackupMetadata>(&content) {
                            // Skip if already in catalog
                            if backups.iter().any(|b| b.id.to_string() == metadata.id) {
                                continue;
                            }

                            if metadata.status != BackupStatus::Completed {
                                continue;
                            }
                            if metadata.backup_type != BackupType::Full
                                && metadata.backup_type != BackupType::Snapshot
                            {
                                continue;
                            }

                            backups.push(BaseBackupInfo {
                                id: Uuid::parse_str(&metadata.id)
                                    .unwrap_or_else(|_| Uuid::new_v4()),
                                path: path.to_string_lossy().to_string(),
                                start_time: metadata.start_time,
                                end_time: metadata.end_time,
                                wal_start: metadata.wal_start,
                                wal_end: metadata.wal_end,
                                server_version: metadata.server_version,
                                size_bytes: metadata.size_bytes,
                                is_remote: false,
                            });
                        }
                    }
                }
            }
        }

        debug!("Found {} local backups", backups.len());
        Ok(backups)
    }

    /// Discover remote backups from storage.
    async fn discover_remote_backups(
        &self,
        storage: &PostgresBackupStorage,
    ) -> Result<Vec<BaseBackupInfo>, PostgresError> {
        let mut backups = Vec::new();

        match storage.list_remote_backups_detailed().await {
            Ok(remote_backups) => {
                for metadata in remote_backups {
                    if metadata.status != BackupStatus::Completed {
                        continue;
                    }
                    if metadata.backup_type != BackupType::Full
                        && metadata.backup_type != BackupType::Snapshot
                    {
                        continue;
                    }

                    backups.push(BaseBackupInfo {
                        id: Uuid::parse_str(&metadata.id).unwrap_or_else(|_| Uuid::new_v4()),
                        path: metadata.id.clone(),
                        start_time: metadata.start_time,
                        end_time: metadata.end_time,
                        wal_start: metadata.wal_start,
                        wal_end: metadata.wal_end,
                        server_version: metadata.server_version,
                        size_bytes: metadata.size_bytes,
                        is_remote: true,
                    });
                }
            }
            Err(e) => {
                warn!("Failed to list remote backups: {}", e);
            }
        }

        debug!("Found {} remote backups", backups.len());
        Ok(backups)
    }

    /// Select the best base backup for the target.
    fn select_base_backup(
        &self,
        backups: &[BaseBackupInfo],
        target: &RecoveryTarget,
    ) -> Result<BaseBackupInfo, PostgresError> {
        let target_time = match target {
            RecoveryTarget::Time(t) => Some(*t),
            RecoveryTarget::Latest => None,
            RecoveryTarget::Lsn(_) | RecoveryTarget::RestorePoint(_) => None,
        };

        // Find the most recent backup that started before the target time
        let suitable_backups: Vec<_> = if let Some(target) = target_time {
            backups.iter().filter(|b| b.start_time <= target).collect()
        } else {
            backups.iter().collect()
        };

        if suitable_backups.is_empty() {
            return Err(PostgresError::BackupError(format!(
                "No backup found that started before target {:?}",
                target
            )));
        }

        // Return the most recent suitable backup
        Ok(suitable_backups[0].clone())
    }

    /// Discover WAL segments from local and remote sources.
    async fn discover_wal_segments(
        &self,
        inventory: &mut WalInventory,
    ) -> Result<(), PostgresError> {
        // Discover local WAL segments
        if let Some(wal_dir) = &self.wal_archive_dir {
            inventory.discover_local(wal_dir)?;
        }

        // Also check pg_wal in backup directories
        if let Ok(entries) = std::fs::read_dir(&self.backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let pg_wal = path.join("pg_wal");
                    if pg_wal.exists() {
                        inventory.discover_local(&pg_wal)?;
                    }
                }
            }
        }

        // Discover remote WAL segments
        if let Some(storage) = &self.storage {
            let wal_prefix = self.wal_prefix.as_deref().unwrap_or("wal/");
            match storage.list_all_objects().await {
                Ok(objects) => {
                    let wal_objects: Vec<RemoteWalObject> = objects
                        .into_iter()
                        .filter(|obj| obj.key.contains(wal_prefix) || obj.key.contains("pg_wal"))
                        .filter_map(|obj| {
                            let filename = obj.key.split('/').next_back()?.to_string();
                            Some(RemoteWalObject {
                                key: obj.key,
                                filename,
                                size: obj.size.unwrap_or(0),
                                last_modified: obj.last_modified,
                            })
                        })
                        .collect();

                    inventory.add_remote_segments(wal_objects);
                }
                Err(e) => {
                    warn!("Failed to list remote WAL segments: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Validate that the target is reachable.
    fn validate_target(
        &self,
        target: &RecoveryTarget,
        base_backup: &BaseBackupInfo,
        wal_inventory: &WalInventory,
    ) -> Result<PlanValidation, PostgresError> {
        let mut validation = PlanValidation::valid();
        let coverage = wal_inventory.calculate_coverage();

        match target {
            RecoveryTarget::Time(target_time) => {
                // Check if target is before backup start
                if *target_time < base_backup.start_time {
                    return Ok(PlanValidation::invalid(vec![format!(
                        "Target time {} is before the base backup start time {}",
                        target_time, base_backup.start_time
                    )]));
                }

                // Check if target is within WAL coverage
                if let Some(latest) = coverage.latest_time {
                    if *target_time > latest {
                        return Ok(PlanValidation::invalid(vec![format!(
                            "Target time {} is beyond available WAL coverage (latest: {})",
                            target_time, latest
                        )]));
                    }
                } else if coverage.segment_count == 0 {
                    return Ok(PlanValidation::invalid(vec![
                        "No WAL segments available for recovery".to_string(),
                    ]));
                }

                // Check for gaps in WAL coverage
                if !coverage.gaps.is_empty() {
                    validation = validation.with_warning(format!(
                        "WAL coverage has {} gap(s) - recovery may fail if target falls within a gap",
                        coverage.gaps.len()
                    ));
                }
            }

            RecoveryTarget::Lsn(lsn) => {
                // Check if LSN is covered
                if !wal_inventory.covers_lsn(lsn) {
                    return Ok(PlanValidation::invalid(vec![format!(
                        "Target LSN {} is not covered by available WAL segments",
                        lsn
                    )]));
                }
            }

            RecoveryTarget::Latest => {
                // Just need some WAL segments
                if coverage.segment_count == 0 {
                    validation = validation.with_warning(
                        "No WAL segments found - recovery will stop at backup end".to_string(),
                    );
                }
            }

            RecoveryTarget::RestorePoint(name) => {
                // Can't validate restore points without replaying WAL
                validation = validation.with_warning(format!(
                    "Cannot validate restore point '{}' without replaying WAL",
                    name
                ));
            }
        }

        Ok(validation)
    }

    /// List available recovery targets (for user guidance).
    pub async fn list_recovery_options(&self) -> Result<RecoveryOptions, PostgresError> {
        let backups = self.discover_base_backups().await?;
        let mut wal_inventory = WalInventory::new();
        self.discover_wal_segments(&mut wal_inventory).await?;

        let coverage = wal_inventory.calculate_coverage();

        let earliest_target = backups.iter().map(|b| b.start_time).min();

        let latest_target = coverage.latest_time;

        Ok(RecoveryOptions {
            available_backups: backups,
            wal_coverage: coverage,
            earliest_recoverable: earliest_target,
            latest_recoverable: latest_target,
        })
    }
}

/// Available recovery options for user guidance.
#[derive(Debug, Clone)]
pub struct RecoveryOptions {
    /// Available base backups.
    pub available_backups: Vec<BaseBackupInfo>,
    /// WAL coverage information.
    pub wal_coverage: super::types::WalCoverage,
    /// Earliest recoverable point.
    pub earliest_recoverable: Option<DateTime<Utc>>,
    /// Latest recoverable point.
    pub latest_recoverable: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_planner_creation() {
        let temp_dir = TempDir::new().unwrap();
        let planner = PitrPlanner::new(temp_dir.path().to_path_buf());
        assert!(planner.storage.is_none());
    }

    #[test]
    fn test_select_base_backup() {
        let temp_dir = TempDir::new().unwrap();
        let planner = PitrPlanner::new(temp_dir.path().to_path_buf());

        let backups = vec![
            BaseBackupInfo {
                id: Uuid::new_v4(),
                path: "/backup1".to_string(),
                start_time: Utc::now() - chrono::Duration::hours(2),
                end_time: Some(Utc::now() - chrono::Duration::hours(1)),
                wal_start: Some("0/1000000".to_string()),
                wal_end: Some("0/2000000".to_string()),
                server_version: "15.0".to_string(),
                size_bytes: 1024,
                is_remote: false,
            },
            BaseBackupInfo {
                id: Uuid::new_v4(),
                path: "/backup2".to_string(),
                start_time: Utc::now() - chrono::Duration::hours(4),
                end_time: Some(Utc::now() - chrono::Duration::hours(3)),
                wal_start: Some("0/0".to_string()),
                wal_end: Some("0/1000000".to_string()),
                server_version: "15.0".to_string(),
                size_bytes: 1024,
                is_remote: false,
            },
        ];

        // Target 30 minutes ago should select the more recent backup
        let target = RecoveryTarget::Time(Utc::now() - chrono::Duration::minutes(30));
        let selected = planner.select_base_backup(&backups, &target).unwrap();
        assert_eq!(selected.path, "/backup1");
    }
}
