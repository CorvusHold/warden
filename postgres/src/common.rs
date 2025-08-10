use chrono::{DateTime, Utc};
use log::info;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Represents a PostgreSQL server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    // SSH tunnel configuration
    pub ssh_host: Option<String>,
    pub ssh_user: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_password: Option<String>,
    pub ssh_key_path: Option<String>,
    pub ssh_local_port: Option<u16>,
    pub ssh_remote_port: Option<u16>,
}

impl PostgresConfig {
    pub fn connection_string(&self) -> String {
        let mut conn_string = format!(
            "host={} port={} dbname={} user={}",
            self.host, self.port, self.database, self.user
        );

        if let Some(password) = &self.password {
            conn_string.push_str(&format!(" password={password}"));
        }

        if let Some(ssl_mode) = &self.ssl_mode {
            conn_string.push_str(&format!(" sslmode={ssl_mode}"));
        }
        info!("Creating connection string for {conn_string}");

        conn_string
    }
}

/// Backup status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupStatus {
    InProgress,
    Completed,
    Failed,
}

/// Backup type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupType {
    Full,
    Incremental,
    Snapshot,
}

/// Represents a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backup {
    pub id: Uuid,
    pub backup_type: BackupType,
    pub status: BackupStatus,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub base_backup_id: Option<Uuid>, // For incremental backups, points to the full backup
    pub wal_start: Option<String>,
    pub wal_end: Option<String>,
    pub size_bytes: Option<u64>,
    pub backup_path: PathBuf,
    pub server_version: String,
    pub error_message: Option<String>,
}

impl Backup {
    pub fn new(
        backup_type: BackupType,
        backup_path: PathBuf,
        server_version: String,
        base_backup_id: Option<Uuid>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            backup_type,
            status: BackupStatus::InProgress,
            start_time: Utc::now(),
            end_time: None,
            base_backup_id,
            wal_start: None,
            wal_end: None,
            size_bytes: None,
            backup_path,
            server_version,
            error_message: None,
        }
    }

    pub fn complete(&mut self, wal_end: String, size_bytes: u64) {
        self.status = BackupStatus::Completed;
        self.end_time = Some(Utc::now());
        self.wal_end = Some(wal_end);
        self.size_bytes = Some(size_bytes);
    }

    pub fn fail(&mut self, error_message: String) {
        self.status = BackupStatus::Failed;
        self.end_time = Some(Utc::now());
        self.error_message = Some(error_message);
    }
}

/// Restore status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreStatus {
    InProgress,
    Completed,
    Failed,
}

/// Represents a restore operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Restore {
    pub id: Uuid,
    pub backup_id: Uuid,
    pub status: RestoreStatus,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub target_time: Option<DateTime<Utc>>, // For point-in-time recovery
    pub restore_path: PathBuf,
    pub error_message: Option<String>,
}

impl Restore {
    pub fn new(backup_id: Uuid, restore_path: PathBuf, target_time: Option<DateTime<Utc>>) -> Self {
        Self {
            id: Uuid::new_v4(),
            backup_id,
            status: RestoreStatus::InProgress,
            start_time: Utc::now(),
            end_time: None,
            target_time,
            restore_path,
            error_message: None,
        }
    }

    pub fn complete(&mut self) {
        self.status = RestoreStatus::Completed;
        self.end_time = Some(Utc::now());
    }

    pub fn fail(&mut self, error_message: String) {
        self.status = RestoreStatus::Failed;
        self.end_time = Some(Utc::now());
        self.error_message = Some(error_message);
    }
}

/// Catalog to manage backups
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupCatalog {
    pub backups: Vec<Backup>,
}

impl BackupCatalog {
    pub fn new() -> Self {
        Self {
            backups: Vec::new(),
        }
    }

    pub fn add_backup(&mut self, backup: Backup) {
        self.backups.push(backup);
    }

    pub fn get_backup(&self, id: &Uuid) -> Option<&Backup> {
        self.backups.iter().find(|b| &b.id == id)
    }

    pub fn get_latest_full_backup(&self) -> Option<&Backup> {
        self.backups
            .iter()
            .filter(|b| b.backup_type == BackupType::Full && b.status == BackupStatus::Completed)
            .max_by_key(|b| b.end_time.unwrap_or(b.start_time))
    }

    pub fn get_incremental_backups_since(&self, base_id: &Uuid) -> Vec<&Backup> {
        self.backups
            .iter()
            .filter(|b| {
                b.backup_type == BackupType::Incremental
                    && b.status == BackupStatus::Completed
                    && b.base_backup_id.as_ref() == Some(base_id)
            })
            .collect()
    }

    pub fn save_to_file(&self, path: &PathBuf) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_from_file(path: &PathBuf) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        let catalog = serde_json::from_str(&json)?;
        Ok(catalog)
    }
}
