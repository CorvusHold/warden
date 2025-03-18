use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info};
use std::fs;
use std::path::{Path, PathBuf};
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

        // Create backup directory if it doesn't exist
        if !self.backup_dir.exists() {
            fs::create_dir_all(&self.backup_dir).map_err(|e| PostgresError::IoError(e))?;
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
        let backup_path = self
            .backup_dir
            .join(format!("snapshot_backup_{}", timestamp));

        fs::create_dir_all(&backup_path).map_err(|e| PostgresError::IoError(e))?;

        let dump_file = backup_path.join(format!("{}.dump", self.config.database));

        let mut backup = Backup::new(
            BackupType::Full, // Snapshots are considered full backups
            backup_path.clone(),
            server_version,
            None,
        );

        // Get current WAL position before backup
        let wal_start = self.get_current_wal_position(&client).await?;
        backup.wal_start = Some(wal_start);

        // Perform the backup using pg_dump
        let options = PgDumpOptions {
            host: self.config.host.clone(),
            port: self.config.port,
            username: self.config.user.clone(),
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

                // Get current WAL position after backup
                let wal_end = self.get_current_wal_position(&client).await?;

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

    /// Get PostgreSQL server version
    async fn get_server_version(&self, client: &Client) -> Result<String, PostgresError> {
        let row = client
            .query_one("SELECT version()", &[])
            .await
            .map_err(|e| PostgresError::PostgresError(e))?;

        let version: String = row.get(0);
        debug!("PostgreSQL server version: {}", version);

        Ok(version)
    }

    /// Get current WAL position
    async fn get_current_wal_position(&self, client: &Client) -> Result<String, PostgresError> {
        let row = client
            .query_one("SELECT pg_current_wal_lsn()", &[])
            .await
            .map_err(|e| PostgresError::PostgresError(e))?;

        let wal_position: String = row.get(0);
        debug!("Current WAL position: {}", wal_position);

        Ok(wal_position)
    }

    /// Calculate backup size in bytes
    fn calculate_backup_size(&self, backup_path: &Path) -> Result<u64, PostgresError> {
        let mut total_size = 0;

        for entry in walkdir::WalkDir::new(backup_path) {
            let entry = entry.map_err(|e| PostgresError::IoError(e.into()))?;
            if entry.file_type().is_file() {
                total_size += entry
                    .metadata()
                    .map_err(|e| PostgresError::IoError(e.into()))?
                    .len();
            }
        }

        debug!("Backup size: {} bytes", total_size);

        Ok(total_size)
    }
}
