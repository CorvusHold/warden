use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info};
use std::fs;
use std::path::{Path, PathBuf};
use tokio_postgres::Client;

use crate::common::{Backup, BackupType, PostgresConfig};
use crate::wrapper::{PgBaseBackup, PgBaseBackupOptions};
use crate::PostgresError;

/// Full backup manager
pub struct FullBackupManager {
    config: PostgresConfig,
    backup_dir: PathBuf,
}

impl FullBackupManager {
    /// Create a new full backup manager
    pub fn new(config: PostgresConfig, backup_dir: PathBuf) -> Self {
        Self { config, backup_dir }
    }

    /// Perform a full backup
    pub async fn backup(&self) -> Result<Backup, PostgresError> {
        info!("Starting full backup");

        // Create backup directory if it doesn't exist
        if !self.backup_dir.exists() {
            info!("Creating base backup directory: {:?}", self.backup_dir);
            fs::create_dir_all(&self.backup_dir).map_err(|e| PostgresError::Io(e))?;
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
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let backup_path = self.backup_dir.join(format!("full_backup_{}", timestamp));

        let mut backup = Backup::new(BackupType::Full, backup_path.clone(), server_version, None);

        // Get current WAL position before backup
        let wal_start = self.get_current_wal_position(&client).await?;
        backup.wal_start = Some(wal_start);

        // Perform the backup using pg_basebackup
        let options = PgBaseBackupOptions {
            host: self.config.host.clone(),
            port: self.config.port,
            username: self.config.user.clone(),
            password: self.config.password.clone().expect("Password is required"),
            pgdata: backup_path.to_string_lossy().to_string(),
            format: "t".to_string(),
            checkpoint: "fast".to_string(),
            wal_method: "stream".to_string(),
            compress: Some("9".to_string()),
            label: Some(format!("full_backup_{}", timestamp)),
            progress: true,
            verbose: true,
        };

        match PgBaseBackup::run(&options) {
            Ok(_) => {
                info!("Physical backup completed successfully");

                // Create a logical backup (SQL dump) of the database
                if let Err(e) = self.create_logical_backup(&backup_path).await {
                    error!("Failed to create logical backup: {}", e);
                    // Continue with the physical backup even if logical backup fails
                } else {
                    info!("Logical backup completed successfully");
                }

                // Get current WAL position after backup
                let wal_end = self.get_current_wal_position(&client).await?;

                // Calculate backup size
                let size_bytes = self.calculate_backup_size(&backup_path)?;

                // Update backup metadata
                backup.complete(wal_end, size_bytes);

                Ok(backup)
            }
            Err(e) => {
                let error_msg = format!("Full backup failed: {}", e);
                error!("{}", error_msg);

                backup.fail(error_msg);

                Err(PostgresError::BackupError("Full backup failed".to_string()))
            }
        }
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
        let row = client
            .query_one("SELECT pg_current_wal_lsn()::TEXT", &[])
            .await
            .map_err(|e| PostgresError::Postgres(e))?;

        let wal_position: String = row.get(0);
        debug!("Current WAL position: {}", wal_position);

        Ok(wal_position)
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

    /// Create a logical backup (SQL dump) of the database
    async fn create_logical_backup(&self, backup_path: &Path) -> Result<(), PostgresError> {
        use std::process::{Command, Stdio};

        info!("Creating logical backup (SQL dump) of the database");

        let db_name = &self.config.database;
        let host = &self.config.host;
        let port = self.config.port;
        let user = &self.config.user;

        // Create dump file path
        let dump_file = backup_path.join(format!("{}.dump", db_name));

        // Use pg_dump to create a custom-format backup
        let result = Command::new("pg_dump")
            .args([
                "-h",
                host,
                "-p",
                &port.to_string(),
                "-U",
                user,
                "-F",
                "c", // custom format
                "-f",
                dump_file.to_str().unwrap(),
                "-v", // verbose
                "-Z",
                "9", // compression level
                db_name,
            ])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| PostgresError::BackupError(format!("Failed to execute pg_dump: {}", e)))?;

        if !result.success() {
            return Err(PostgresError::BackupError(
                "pg_dump command failed".to_string(),
            ));
        }

        // Also create a plain SQL backup for flexibility
        let sql_file = backup_path.join(format!("{}.sql", db_name));

        let result = Command::new("pg_dump")
            .args([
                "-h",
                host,
                "-p",
                &port.to_string(),
                "-U",
                user,
                "-F",
                "p", // plain format
                "-f",
                sql_file.to_str().unwrap(),
                db_name,
            ])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| {
                PostgresError::BackupError(format!("Failed to execute pg_dump for SQL: {}", e))
            })?;

        if !result.success() {
            return Err(PostgresError::BackupError(
                "pg_dump SQL command failed".to_string(),
            ));
        }

        info!(
            "Logical backup created successfully at {:?} and {:?}",
            dump_file, sql_file
        );
        Ok(())
    }
}
