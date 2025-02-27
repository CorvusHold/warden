pub mod full;
pub mod incremental;
pub mod snapshot;

use async_trait::async_trait;
use std::path::PathBuf;

use crate::common::{Backup, BackupCatalog, PostgresConfig};
use crate::PostgresError;

/// Trait for backup managers
#[async_trait]
pub trait BackupManager {
    /// Perform a backup
    async fn backup(&self) -> Result<Backup, PostgresError>;
}

/// Factory for creating backup managers
pub struct BackupManagerFactory;

impl BackupManagerFactory {
    /// Create a full backup manager
    pub fn create_full_backup_manager(
        config: PostgresConfig,
        backup_dir: PathBuf,
    ) -> full::FullBackupManager {
        full::FullBackupManager::new(config, backup_dir)
    }

    /// Create an incremental backup manager
    pub fn create_incremental_backup_manager(
        config: PostgresConfig,
        backup_dir: PathBuf,
        catalog: BackupCatalog,
    ) -> incremental::IncrementalBackupManager {
        incremental::IncrementalBackupManager::new(config, backup_dir, catalog)
    }

    /// Create a snapshot backup manager
    pub fn create_snapshot_backup_manager(
        config: PostgresConfig,
        backup_dir: PathBuf,
    ) -> snapshot::SnapshotBackupManager {
        snapshot::SnapshotBackupManager::new(config, backup_dir)
    }
}
