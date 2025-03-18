use anyhow::Result;
use chrono::Utc;
use log::{error, info};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::common::{Backup, PostgresConfig, Restore};
use crate::PostgresError;

/// Full restore manager
pub struct FullRestoreManager {
    config: PostgresConfig,
    backup: Backup,
    target_dir: PathBuf,
}

impl FullRestoreManager {
    /// Create a new full restore manager
    pub fn new(config: PostgresConfig, backup: Backup, target_dir: PathBuf) -> Self {
        Self {
            config,
            backup,
            target_dir,
        }
    }

    /// Perform a full restore
    pub async fn restore(&self) -> Result<Restore, PostgresError> {
        info!("Starting full restore from backup: {}", self.backup.id);

        // Create restore metadata
        let mut restore = Restore::new(self.backup.id, self.target_dir.clone(), None);

        // Create target directory if it doesn't exist
        if !self.target_dir.exists() {
            fs::create_dir_all(&self.target_dir).map_err(|e| PostgresError::IoError(e))?;
        }

        // Check if the backup exists
        if !self.backup.backup_path.exists() {
            let error_msg = format!("Backup path does not exist: {:?}", self.backup.backup_path);
            error!("{}", error_msg);

            restore.fail(error_msg);
            return Err(PostgresError::RestoreError(
                "Backup path does not exist".to_string(),
            ));
        }

        // Copy backup files to target directory
        match self.copy_backup_files() {
            Ok(_) => {
                info!("Backup files copied successfully");

                // Create recovery.conf file
                self.create_recovery_conf()?;

                // Update restore metadata
                restore.complete();

                info!("Full restore completed successfully");
                Ok(restore)
            }
            Err(e) => {
                let error_msg = format!("Full restore failed: {}", e);
                error!("{}", error_msg);

                restore.fail(error_msg);

                Err(PostgresError::RestoreError(
                    "Full restore failed".to_string(),
                ))
            }
        }
    }

    /// Copy backup files to target directory
    fn copy_backup_files(&self) -> Result<(), PostgresError> {
        info!(
            "Copying backup files from {:?} to {:?}",
            self.backup.backup_path, self.target_dir
        );

        // Use rsync or similar tool for efficient copying
        // For simplicity, we'll use a simple recursive copy here
        let output = Command::new("cp")
            .arg("-R")
            .arg(format!("{}/*", self.backup.backup_path.to_string_lossy()))
            .arg(&self.target_dir)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| PostgresError::IoError(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(PostgresError::RestoreError(format!(
                "Failed to copy backup files: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Create recovery.conf file for PostgreSQL
    fn create_recovery_conf(&self) -> Result<(), PostgresError> {
        let recovery_conf_path = self.target_dir.join("recovery.conf");

        let recovery_conf_content = format!(
            "# Recovery configuration file created by Warden\n\
             # Created at: {}\n\
             restore_command = 'cp {}/pg_wal/%f %p'\n\
             recovery_target_timeline = 'latest'\n",
            Utc::now(),
            self.backup.backup_path.to_string_lossy()
        );

        fs::write(&recovery_conf_path, recovery_conf_content)
            .map_err(|e| PostgresError::IoError(e))?;

        info!("Created recovery.conf file at {:?}", recovery_conf_path);

        Ok(())
    }
}
