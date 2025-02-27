use chrono::Utc;
use log::{debug, error, info};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::common::{Backup, BackupType, PostgresConfig, Restore, RestoreStatus};
use crate::wrapper::pg_restore::PgRestore;
use crate::PostgresError;

/// Manager for restoring snapshot backups
pub struct SnapshotRestoreManager {
    config: PostgresConfig,
    backup: Backup,
    target_dir: PathBuf,
}

impl SnapshotRestoreManager {
    /// Create a new snapshot restore manager
    pub fn new(
        config: PostgresConfig,
        backup: Backup,
        target_dir: PathBuf,
    ) -> Self {
        // Verify backup type is handled in the factory method
        
        // Create target directory if it doesn't exist
        if !target_dir.exists() {
            let _ = fs::create_dir_all(&target_dir);
        }

        Self {
            config,
            backup,
            target_dir,
        }
    }

    /// Restore a snapshot backup
    pub async fn restore(&self) -> Result<Restore, PostgresError> {
        info!("Starting snapshot restore for backup: {}", self.backup.id);

        let start_time = Utc::now();
        let restore_id = Uuid::new_v4();

        // Create restore object
        let mut restore = Restore {
            id: restore_id,
            backup_id: self.backup.id,
            status: RestoreStatus::InProgress,
            start_time,
            end_time: None,
            target_time: None,
            restore_path: self.target_dir.clone(),
            error_message: None,
        };

        // Get snapshot file path
        let snapshot_file =
            Path::new(&self.backup.backup_path).join(format!("snapshot_{}.sql", self.backup.id));

        if !snapshot_file.exists() {
            let error_msg = format!("Snapshot file does not exist: {:?}", snapshot_file);
            error!("{}", error_msg);

            restore.status = RestoreStatus::Failed;
            restore.end_time = Some(Utc::now());
            restore.error_message = Some(error_msg);

            return Err(PostgresError::RestoreError(format!(
                "Snapshot file does not exist: {:?}",
                snapshot_file
            )));
        }

        // Create pg_restore wrapper
        let pg_restore = PgRestore::new(self.config.clone());

        // Restore database from snapshot
        match pg_restore.restore(&snapshot_file, None).await {
            Ok(_) => {
                info!("Snapshot restore completed successfully");

                restore.status = RestoreStatus::Completed;
                restore.end_time = Some(Utc::now());
            }
            Err(e) => {
                let error_msg = format!("Snapshot restore failed: {}", e);
                error!("{}", error_msg);

                restore.status = RestoreStatus::Failed;
                restore.end_time = Some(Utc::now());
                restore.error_message = Some(error_msg);

                return Err(e);
            }
        }

        Ok(restore)
    }

    /// List the contents of a snapshot backup
    pub async fn list_contents(&self) -> Result<String, PostgresError> {
        info!("Listing contents of snapshot backup: {}", self.backup.id);

        // Get snapshot file path
        let snapshot_file =
            Path::new(&self.backup.backup_path).join(format!("snapshot_{}.sql", self.backup.id));

        if !snapshot_file.exists() {
            return Err(PostgresError::RestoreError(format!(
                "Snapshot file does not exist: {:?}",
                snapshot_file
            )));
        }

        // Create pg_restore wrapper
        let pg_restore = PgRestore::new(self.config.clone());

        // List contents of snapshot
        pg_restore.list_contents(&snapshot_file).await
    }
}
