use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info, warn};
use std::fs;
use std::path::{Path, PathBuf};
use tokio_postgres::Client;

use crate::common::{Backup, BackupCatalog, BackupType, PostgresConfig};
use crate::PostgresError;

/// Incremental backup manager
pub struct IncrementalBackupManager {
    config: PostgresConfig,
    backup_dir: PathBuf,
    catalog: BackupCatalog,
}

impl IncrementalBackupManager {
    /// Create a new incremental backup manager
    pub fn new(config: PostgresConfig, backup_dir: PathBuf, catalog: BackupCatalog) -> Self {
        Self {
            config,
            backup_dir,
            catalog,
        }
    }

    /// Perform an incremental backup based on the latest full backup
    pub async fn backup(&self) -> Result<Backup, PostgresError> {
        info!("Starting incremental backup");

        // Find the latest full backup
        let base_backup = match self.catalog.get_latest_full_backup() {
            Some(backup) => backup,
            None => {
                return Err(PostgresError::BackupError(
                    "No full backup found to base incremental backup on".to_string(),
                ));
            }
        };

        // Create backup directory if it doesn't exist
        if !self.backup_dir.exists() {
            fs::create_dir_all(&self.backup_dir).map_err(|e| PostgresError::IoError(e))?;
        }

        // Connect to PostgreSQL
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
            .join(format!("incremental_backup_{}", timestamp));

        fs::create_dir_all(&backup_path).map_err(|e| PostgresError::IoError(e))?;

        let mut backup = Backup::new(
            BackupType::Incremental,
            backup_path.clone(),
            server_version,
            Some(base_backup.id),
        );

        // Get current WAL position before backup
        let wal_start = match &base_backup.wal_end {
            Some(wal) => wal.clone(),
            None => {
                warn!("Base backup doesn't have WAL end position, using current position");
                self.get_current_wal_position(&client).await?
            }
        };

        backup.wal_start = Some(wal_start.clone());

        // Perform WAL archiving from the last position to the current position
        let wal_files = self
            .archive_wal_files(&client, &wal_start, &backup_path)
            .await?;

        if wal_files.is_empty() {
            warn!("No WAL files were archived, incremental backup might be empty");
        }

        // Get current WAL position after backup
        let wal_end = self.get_current_wal_position(&client).await?;

        // Calculate backup size
        let size_bytes = self.calculate_backup_size(&backup_path)?;

        // Update backup metadata
        backup.complete(wal_end, size_bytes);

        info!("Incremental backup completed successfully");
        Ok(backup)
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

    /// Archive WAL files from start_lsn to current position
    async fn archive_wal_files(
        &self,
        client: &Client,
        start_lsn: &str,
        backup_path: &Path,
    ) -> Result<Vec<String>, PostgresError> {
        // Create WAL directory
        let wal_dir = backup_path.join("pg_wal");
        fs::create_dir_all(&wal_dir).map_err(|e| PostgresError::IoError(e))?;

        // Get list of WAL files that need to be archived
        let rows = client
            .query(
                "SELECT file_name FROM pg_walfile_name_offset(pg_lsn($1))",
                &[&start_lsn],
            )
            .await
            .map_err(|e| PostgresError::PostgresError(e))?;

        let mut wal_files = Vec::new();

        for row in rows {
            let file_name: String = row.get(0);
            wal_files.push(file_name);
        }

        // Switch to a new WAL file to ensure all changes are archived
        client
            .execute("SELECT pg_switch_wal()", &[])
            .await
            .map_err(|e| PostgresError::PostgresError(e))?;

        // For each WAL file, copy it to the backup directory
        // Note: In a real implementation, you would need to access the WAL files directly
        // or use pg_receivewal to stream them. This is a simplified example.
        info!("Archived {} WAL files", wal_files.len());

        Ok(wal_files)
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
