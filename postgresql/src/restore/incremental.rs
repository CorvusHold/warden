use anyhow::Result;
use chrono::Utc;
use log::{error, info};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::common::{Backup, PostgresConfig, Restore};
use crate::PostgresError;

/// Incremental restore manager
pub struct IncrementalRestoreManager {
    config: PostgresConfig,
    full_backup: Backup,
    incremental_backups: Vec<Backup>,
    target_dir: PathBuf,
}

impl IncrementalRestoreManager {
    /// Create a new incremental restore manager
    pub fn new(
        config: PostgresConfig,
        full_backup: Backup,
        incremental_backups: Vec<Backup>,
        target_dir: PathBuf,
    ) -> Self {
        Self {
            config,
            full_backup,
            incremental_backups,
            target_dir,
        }
    }

    /// Perform an incremental restore
    pub async fn restore(&self) -> Result<Restore, PostgresError> {
        info!(
            "Starting incremental restore from full backup: {} and {} incremental backups",
            self.full_backup.id,
            self.incremental_backups.len()
        );

        // Create restore metadata
        let mut restore = Restore::new(self.full_backup.id, self.target_dir.clone(), None);

        // Create target directory if it doesn't exist
        if !self.target_dir.exists() {
            fs::create_dir_all(&self.target_dir).map_err(|e| PostgresError::IoError(e))?;
        }

        // Check if the full backup exists
        if !self.full_backup.backup_path.exists() {
            let error_msg = format!(
                "Full backup path does not exist: {:?}",
                self.full_backup.backup_path
            );
            error!("{}", error_msg);

            restore.fail(error_msg);
            return Err(PostgresError::RestoreError(
                "Full backup path does not exist".to_string(),
            ));
        }

        // First restore the full backup
        match self.restore_full_backup() {
            Ok(_) => {
                info!("Full backup restored successfully");

                // Then apply incremental backups
                match self.apply_incremental_backups() {
                    Ok(_) => {
                        info!("Incremental backups applied successfully");

                        // Create recovery.conf file
                        self.create_recovery_conf()?;

                        // Update restore metadata
                        restore.complete();

                        info!("Incremental restore completed successfully");
                        Ok(restore)
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to apply incremental backups: {}", e);
                        error!("{}", error_msg);

                        restore.fail(error_msg);

                        Err(PostgresError::RestoreError(
                            "Failed to apply incremental backups".to_string(),
                        ))
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to restore full backup: {}", e);
                error!("{}", error_msg);

                restore.fail(error_msg);

                Err(PostgresError::RestoreError(
                    "Failed to restore full backup".to_string(),
                ))
            }
        }
    }

    /// Restore the full backup
    fn restore_full_backup(&self) -> Result<(), PostgresError> {
        info!(
            "Restoring full backup from {:?} to {:?}",
            self.full_backup.backup_path, self.target_dir
        );

        // Use rsync or similar tool for efficient copying
        // For simplicity, we'll use a simple recursive copy here
        let output = Command::new("cp")
            .arg("-R")
            .arg(format!(
                "{}/*",
                self.full_backup.backup_path.to_string_lossy()
            ))
            .arg(&self.target_dir)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| PostgresError::IoError(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PostgresError::RestoreError(format!(
                "Failed to copy full backup files: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Apply incremental backups
    fn apply_incremental_backups(&self) -> Result<(), PostgresError> {
        // Create WAL directory if it doesn't exist
        let wal_dir = self.target_dir.join("pg_wal");
        if !wal_dir.exists() {
            fs::create_dir_all(&wal_dir).map_err(|e| PostgresError::IoError(e))?;
        }

        // Sort incremental backups by start time
        let mut sorted_backups = self.incremental_backups.clone();
        sorted_backups.sort_by_key(|b| b.start_time);

        // Apply each incremental backup
        for (i, backup) in sorted_backups.iter().enumerate() {
            info!(
                "Applying incremental backup {} of {}: {}",
                i + 1,
                sorted_backups.len(),
                backup.id
            );

            // Check if the backup exists
            if !backup.backup_path.exists() {
                return Err(PostgresError::RestoreError(format!(
                    "Incremental backup path does not exist: {:?}",
                    backup.backup_path
                )));
            }

            // Copy WAL files from incremental backup to target WAL directory
            let backup_wal_dir = backup.backup_path.join("pg_wal");
            if backup_wal_dir.exists() {
                let output = Command::new("cp")
                    .arg("-R")
                    .arg(format!("{}/*", backup_wal_dir.to_string_lossy()))
                    .arg(&wal_dir)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .output()
                    .map_err(|e| PostgresError::IoError(e))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(PostgresError::RestoreError(format!(
                        "Failed to copy WAL files from incremental backup: {}",
                        stderr
                    )));
                }
            } else {
                info!(
                    "No WAL directory found in incremental backup: {:?}",
                    backup.backup_path
                );
            }
        }

        Ok(())
    }

    /// Create recovery.conf file for PostgreSQL
    fn create_recovery_conf(&self) -> Result<(), PostgresError> {
        let recovery_conf_path = self.target_dir.join("recovery.conf");

        // Get the last incremental backup for the WAL end position
        let last_backup = self
            .incremental_backups
            .iter()
            .max_by_key(|b| b.end_time.unwrap_or(b.start_time))
            .unwrap_or(&self.full_backup);

        let recovery_conf_content = format!(
            "# Recovery configuration file created by Warden\n\
             # Created at: {}\n\
             restore_command = 'cp {}/pg_wal/%f %p'\n\
             recovery_target_timeline = 'latest'\n",
            Utc::now(),
            self.target_dir.to_string_lossy()
        );

        fs::write(&recovery_conf_path, recovery_conf_content)
            .map_err(|e| PostgresError::IoError(e))?;

        info!("Created recovery.conf file at {:?}", recovery_conf_path);

        Ok(())
    }
}
