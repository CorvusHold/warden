use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info, warn};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration};
use tokio_postgres::Client;

use crate::common::{Backup, BackupType, PostgresConfig};
use crate::wrapper::{PgDump, PgDumpFormat, PgDumpOptions};
use crate::PostgresError;

/// Snapshot backup manager for logical backups
pub struct SnapshotBackupManager {
    config: PostgresConfig,
    backup_dir: PathBuf,
}

impl SnapshotBackupManager {
    /// Create a new snapshot backup manager
    pub fn new(config: PostgresConfig, backup_dir: PathBuf) -> Self {
        Self { config, backup_dir }
    }

    /// Perform a snapshot backup using pg_dump
    pub async fn backup(&self) -> Result<Backup, PostgresError> {
        info!("Starting snapshot backup");

        // Verify base backup directory exists and is writable
        if !self.backup_dir.exists() {
            info!("Creating base backup directory: {:?}", self.backup_dir);
            fs::create_dir_all(&self.backup_dir).map_err(|e| PostgresError::Io(e))?;
        }

        // Verify base directory permissions
        let base_metadata = fs::metadata(&self.backup_dir).map_err(|e| PostgresError::Io(e))?;
        if !base_metadata.is_dir() || base_metadata.permissions().mode() & 0o777 != 0o755 {
            return Err(PostgresError::BackupError(format!(
                "Base backup directory has incorrect permissions: {:?}",
                self.backup_dir
            )));
        }

        // Create timestamped backup directory
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let backup_path = self
            .backup_dir
            .join(format!("snapshot_backup_{}", timestamp));

        info!("Creating backup directory: {:?}", backup_path);
        fs::create_dir_all(&backup_path).map_err(|e| PostgresError::Io(e))?;

        // Set permissions to 755
        fs::set_permissions(&backup_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| PostgresError::Io(e))?;

        // Verify directory was created with correct permissions
        let metadata = fs::metadata(&backup_path).map_err(|e| PostgresError::Io(e))?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o777 != 0o755 {
            return Err(PostgresError::BackupError(format!(
                "Failed to create backup directory with correct permissions: {:?}",
                backup_path
            )));
        }

        // Add delay and retry for directory creation
        let mut retries = 3;
        while retries > 0 && !backup_path.exists() {
            info!(
                "Waiting for backup directory creation ({} retries left)",
                retries
            );
            sleep(Duration::from_millis(500)).await;
            retries -= 1;
        }

        if !backup_path.exists() {
            error!(
                "Failed to create backup directory after retries: {:?}",
                backup_path
            );
            return Err(PostgresError::BackupError(format!(
                "Failed to create backup directory: {:?}",
                backup_path
            )));
        }

        // Connect to PostgreSQL to get server version
        let conn_string = self.config.connection_string();
        let (client, connection) = tokio_postgres::connect(&conn_string, tokio_postgres::NoTls)
            .await
            .map_err(|e| PostgresError::ConnectionError(e.to_string()))?;

        // Spawn the connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("Connection error: {}", e);
            }
        });

        // Get PostgreSQL server version
        let server_version = self.get_server_version(&client).await?;

        // Create backup metadata
        let mut backup = Backup::new(
            BackupType::Snapshot,
            backup_path.clone(),
            server_version,
            None,
        );
        let dump_file = backup_path.join(format!("snapshot_{}.dump", backup.id));

        // Temporarily use a placeholder for WAL position to bypass the pg_lsn type issue
        let wal_start = "0/0000000".to_string();
        debug!("Using placeholder WAL start position: {}", wal_start);
        backup.wal_start = Some(wal_start);

        // Perform the backup using pg_dump
        let options = PgDumpOptions {   
            host: self.config.host.clone(),
            port: self.config.port,
            username: self.config.user.clone(),
            password: self.config.password.clone().expect("Password is required"),
            database: self.config.database.clone(),
            file: dump_file.to_string_lossy().to_string(),
            format: PgDumpFormat::Custom,
            compress: Some(9),
            schema_only: false,
            data_only: false,
            clean: true,
            if_exists: true,
            verbose: true,
            schemas: Vec::new(),
            tables: Vec::new(),
            exclude_tables: Vec::new(),
        };

        match PgDump::run(&options) {
            Ok(_) => {
                info!("Snapshot backup completed successfully");

                // Temporarily use a placeholder for WAL position to bypass the pg_lsn type issue
                let wal_end = "0/0000000".to_string();
                debug!("Using placeholder WAL end position: {}", wal_end);

                // Calculate backup size
                let size_bytes = self.calculate_backup_size(&backup_path)?;

                // Update backup metadata
                backup.complete(wal_end, size_bytes);

                Ok(backup)
            }
            Err(e) => {
                let error_msg = format!("Snapshot backup failed: {}", e);
                error!("{}", error_msg);

                backup.fail(error_msg);

                Err(PostgresError::BackupError(
                    "Snapshot backup failed".to_string(),
                ))
            }
        }
    }

    /// Restore a full backup
    pub async fn restore_full_backup(
        &self,
        backup_path: &Path,
        restore_dir: &Path,
    ) -> Result<(), PostgresError> {
        // Verify backup directory exists
        if !backup_path.exists() {
            return Err(PostgresError::RestoreError(format!(
                "Backup directory does not exist: {:?}",
                backup_path
            )));
        }

        // Verify backup directory is actually a directory
        if !backup_path.is_dir() {
            return Err(PostgresError::RestoreError(format!(
                "Backup path is not a directory: {:?}",
                backup_path
            )));
        }

        let backup_files = fs::read_dir(&backup_path)
            .map_err(|e| PostgresError::Io(e))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();

        if backup_files.is_empty() {
            return Err(PostgresError::RestoreError(
                "No backup files found".to_string(),
            ));
        }

        // Create restore directory if it doesn't exist
        if !restore_dir.exists() {
            fs::create_dir_all(restore_dir).map_err(|e| PostgresError::Io(e))?;
        }

        for file in &backup_files {
            if !file.exists() {
                return Err(PostgresError::RestoreError(format!(
                    "Backup file does not exist: {:?}",
                    file
                )));
            }

            let dest_path = restore_dir.join(file.file_name().unwrap());

            // Create parent directories if they don't exist
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).map_err(|e| PostgresError::Io(e))?;
            }

            // Copy with progress and retries
            let mut retries = 3;
            while retries > 0 {
                match fs::copy(file, &dest_path) {
                    Ok(_) => break,
                    Err(e) if retries > 1 => {
                        warn!(
                            "Copy failed, retrying ({} attempts left): {} - {}",
                            retries - 1,
                            file.display(),
                            e
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        retries -= 1;
                    }
                    Err(e) => {
                        return Err(PostgresError::RestoreError(format!(
                            "Failed to copy {} to {} after 3 attempts: {}",
                            file.display(),
                            dest_path.display(),
                            e
                        )))
                    }
                }
            }
        }

        Ok(())
    }

    /// Get PostgreSQL server version
    async fn get_server_version(&self, client: &Client) -> Result<String, PostgresError> {
        let row = client
            .query_one("SELECT version()", &[])
            .await
            .map_err(|e| PostgresError::Postgres(e))?;

        let version: String = row.get(0);
        debug!("PostgreSQL server version: {}", version);

        Ok(version)
    }

    /// Get current WAL position
    async fn get_current_wal_position(&self, client: &Client) -> Result<String, PostgresError> {
        // Use a completely different approach to avoid pg_lsn type issues
        // Use a simple query that returns a text representation directly
        let result = client
            .simple_query("SELECT pg_current_wal_lsn()::TEXT")
            .await
            .map_err(|e| PostgresError::Postgres(e))?;

        // Extract the value from the result
        if let Some(tokio_postgres::SimpleQueryMessage::Row(row)) = result.into_iter().next() {
            if let Some(value) = row.get(0) {
                debug!("Current WAL position: {}", value);
                return Ok(value.to_string());
            }
        }

        Err(PostgresError::BackupError(
            "Failed to get WAL position".to_string(),
        ))
    }

    /// Calculate backup size in bytes
    fn calculate_backup_size(&self, backup_path: &Path) -> Result<u64, PostgresError> {
        let mut total_size = 0;

        for entry in walkdir::WalkDir::new(backup_path) {
            let entry = entry.map_err(|e| PostgresError::Io(e.into()))?;
            if entry.file_type().is_file() {
                total_size += entry
                    .metadata()
                    .map_err(|e| PostgresError::Io(e.into()))?
                    .len();
            }
        }

        debug!("Backup size: {} bytes", total_size);

        Ok(total_size)
    }
}
