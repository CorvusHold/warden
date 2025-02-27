pub mod full;
pub mod incremental;
pub mod point_in_time;
pub mod snapshot;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::PathBuf;

use crate::common::{Backup, BackupCatalog, PostgresConfig, Restore, BackupType};
use crate::PostgresError;

/// Trait for restore managers
#[async_trait]
pub trait RestoreManager {
    /// Perform a restore
    async fn restore(&self) -> Result<Restore, PostgresError>;
}

/// Factory for creating restore managers
pub struct RestoreManagerFactory;

impl RestoreManagerFactory {
    /// Create a full restore manager
    pub fn create_full_restore_manager(
        config: PostgresConfig,
        backup: Backup,
        target_dir: PathBuf,
    ) -> full::FullRestoreManager {
        full::FullRestoreManager::new(config, backup, target_dir)
    }

    /// Create an incremental restore manager
    pub fn create_incremental_restore_manager(
        config: PostgresConfig,
        full_backup: Backup,
        incremental_backups: Vec<Backup>,
        target_dir: PathBuf,
    ) -> incremental::IncrementalRestoreManager {
        incremental::IncrementalRestoreManager::new(
            config,
            full_backup,
            incremental_backups,
            target_dir,
        )
    }

    /// Create a point-in-time restore manager
    pub fn create_point_in_time_restore_manager(
        config: PostgresConfig,
        full_backup: Backup,
        incremental_backups: Vec<Backup>,
        target_dir: PathBuf,
        target_time: DateTime<Utc>,
    ) -> point_in_time::PointInTimeRestoreManager {
        point_in_time::PointInTimeRestoreManager::new(
            config,
            full_backup,
            incremental_backups,
            target_dir,
            target_time,
        )
    }

    /// Create a snapshot restore manager
    pub fn create_snapshot_restore_manager(
        config: PostgresConfig,
        backup: Backup,
        target_dir: PathBuf,
    ) -> Result<snapshot::SnapshotRestoreManager, PostgresError> {
        // Verify backup type
        if backup.backup_type != BackupType::Snapshot {
            return Err(PostgresError::RestoreError(format!(
                "Backup {} is not a snapshot backup",
                backup.id
            )));
        }
        
        Ok(snapshot::SnapshotRestoreManager::new(config, backup, target_dir))
    }
}
