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
            fs::create_dir_all(&self.backup_dir).map_err(PostgresError::Io)?;
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

        fs::create_dir_all(&backup_path).map_err(PostgresError::Io)?;

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
            .map_err(PostgresError::Postgres)?;

        let version: String = row.get(0);
        debug!("PostgreSQL server version: {}", version);

        Ok(version)
    }

    /// Get current WAL position
    async fn get_current_wal_position(&self, client: &Client) -> Result<String, PostgresError> {
        let row = client
            .query_one("SELECT pg_current_wal_lsn()::TEXT", &[])
            .await
            .map_err(PostgresError::Postgres)?;

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
        fs::create_dir_all(&wal_dir).map_err(PostgresError::Io)?;

        // Get the PostgreSQL data directory
        let data_dir_rows = client
            .query(
                "SELECT setting FROM pg_settings WHERE name = 'data_directory'",
                &[],
            )
            .await
            .map_err(PostgresError::Postgres)?;

        if data_dir_rows.is_empty() {
            return Err(PostgresError::BackupError(
                "Could not determine PostgreSQL data directory".to_string(),
            ));
        }

        let data_dir: String = data_dir_rows[0].get(0);
        let pg_wal_dir = Path::new(&data_dir).join("pg_wal");

        // Get current WAL position
        let current_lsn_rows = client
            .query("SELECT pg_current_wal_lsn()::text", &[])
            .await
            .map_err(PostgresError::Postgres)?;

        let current_lsn: String = current_lsn_rows[0].get(0);

        // Get list of WAL files between start_lsn and current_lsn
        let query = format!(
            "SELECT file_name FROM pg_walfile_name_offset('{}') 
                            UNION 
                            SELECT file_name FROM pg_walfile_name_offset('{}')",
            start_lsn, current_lsn
        );

        let rows = client
            .query(&query, &[])
            .await
            .map_err(PostgresError::Postgres)?;

        // Switch to a new WAL file to ensure all changes are archived
        client
            .execute("SELECT pg_switch_wal()", &[])
            .await
            .map_err(PostgresError::Postgres)?;

        // Get all available WAL files in the pg_wal directory
        let mut available_wal_files = Vec::new();
        if let Ok(entries) = fs::read_dir(&pg_wal_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(file_name) = path.file_name() {
                        if let Some(file_name_str) = file_name.to_str() {
                            // Only include files that match the WAL file naming pattern
                            if file_name_str.len() == 24 && file_name_str.starts_with("0000000") {
                                available_wal_files.push(file_name_str.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Collect WAL files from the query results
        let mut needed_wal_files = Vec::new();
        for row in rows {
            let file_name: String = row.get(0);
            needed_wal_files.push(file_name);
        }

        // Add the current WAL file
        let current_wal_rows = client
            .query("SELECT pg_walfile_name(pg_current_wal_lsn())", &[])
            .await
            .map_err(PostgresError::Postgres)?;

        if !current_wal_rows.is_empty() {
            let current_wal: String = current_wal_rows[0].get(0);
            needed_wal_files.push(current_wal);
        }

        // Copy available WAL files to the backup directory
        let mut archived_files = Vec::new();
        for file_name in &available_wal_files {
            let source_path = pg_wal_dir.join(file_name);
            let target_path = wal_dir.join(file_name);

            // Copy the WAL file to the backup directory
            match fs::copy(&source_path, &target_path) {
                Ok(_) => {
                    debug!(
                        "Copied WAL file: {} -> {}",
                        source_path.display(),
                        target_path.display()
                    );
                    archived_files.push(file_name.clone());
                }
                Err(e) => {
                    warn!("Failed to copy WAL file {}: {}", source_path.display(), e);
                }
            }
        }

        info!("Archived {} WAL files", archived_files.len());

        Ok(archived_files)
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
