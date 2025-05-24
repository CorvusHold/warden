use chrono::{DateTime, Utc};
use log::{info, warn};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::backup::BackupManagerFactory;
use crate::common::{Backup, BackupCatalog, BackupType, PostgresConfig, Restore};
use crate::restore::RestoreManagerFactory;

use crate::PostgresError;

/// Main manager for PostgreSQL backup and restore operations
pub struct PostgresManager {
    pub config: PostgresConfig,
    backup_dir: PathBuf,
    catalog_path: PathBuf,
    catalog: BackupCatalog,
}

impl PostgresManager {
    pub fn new(config: PostgresConfig, backup_dir: PathBuf) -> Result<Self, PostgresError> {
        // Create backup directory if it doesn't exist
        if !backup_dir.exists() {
            fs::create_dir_all(&backup_dir).map_err(PostgresError::Io)?;
        }

        let catalog_path = backup_dir.join("backup_catalog.json");

        // Load or create catalog
        let catalog = if catalog_path.exists() {
            match BackupCatalog::load_from_file(&catalog_path) {
                Ok(catalog) => {
                    info!(
                        "Loaded backup catalog with {} backups",
                        catalog.backups.len()
                    );
                    catalog
                }
                Err(e) => {
                    warn!("Failed to load backup catalog: {}, creating a new one", e);
                    let catalog = BackupCatalog::new();
                    if let Err(e) = catalog.save_to_file(&catalog_path) {
                        warn!("Failed to save new backup catalog: {}", e);
                    }
                    catalog
                }
            }
        } else {
            info!("Creating new backup catalog");
            let catalog = BackupCatalog::new();
            if let Err(e) = catalog.save_to_file(&catalog_path) {
                warn!("Failed to save new backup catalog: {}", e);
            }
            catalog
        };

        let manager = Self {
            config,
            backup_dir,
            catalog_path,
            catalog,
        };
        Ok(manager)
    }

    /// Perform a full backup
    pub async fn full_backup(&mut self) -> Result<Backup, PostgresError> {
        info!("Starting full backup");

        let manager = BackupManagerFactory::create_full_backup_manager(
            self.config.clone(),
            self.backup_dir.clone(),
        );

        // Perform the backup operation
        let backup = manager.backup().await?;

        // Add backup to catalog
        self.catalog.add_backup(backup.clone());

        // Save catalog
        self.save_catalog()?;

        info!("Full backup completed: {}", backup.id);
        Ok(backup)
    }

    /// Perform an incremental backup
    pub async fn incremental_backup(&mut self) -> Result<Backup, PostgresError> {
        info!("Starting incremental backup");

        let manager = BackupManagerFactory::create_incremental_backup_manager(
            self.config.clone(),
            self.backup_dir.clone(),
            self.catalog.clone(),
        );

        // Perform the backup operation
        let backup = manager.backup().await?;

        // Add backup to catalog
        self.catalog.add_backup(backup.clone());

        // Save catalog
        self.save_catalog()?;

        info!("Incremental backup completed: {}", backup.id);
        Ok(backup)
    }

    /// Perform a snapshot backup
    pub async fn snapshot_backup(&mut self) -> Result<Backup, PostgresError> {
        info!("Starting snapshot backup");

        let manager = BackupManagerFactory::create_snapshot_backup_manager(
            self.config.clone(),
            self.backup_dir.clone(),
        );

        // Perform the backup operation
        let backup = manager.backup().await?;

        // Add backup to catalog
        self.catalog.add_backup(backup.clone());

        // Save catalog
        self.save_catalog()?;

        info!("Snapshot backup completed: {}", backup.id);
        Ok(backup)
    }

    /// Restore from a full backup
    pub async fn restore_full_backup(
        &mut self,
        backup_id: &Uuid,
        target_dir: PathBuf,
    ) -> Result<Restore, PostgresError> {
        info!("Starting restore from full backup: {}", backup_id);

        // Find the backup
        let backup = match self.catalog.get_backup(backup_id) {
            Some(backup) => {
                if backup.backup_type != BackupType::Full {
                    return Err(PostgresError::RestoreError(format!(
                        "Backup {} is not a full backup",
                        backup_id
                    )));
                }
                backup.clone()
            }
            None => {
                return Err(PostgresError::RestoreError(format!(
                    "Backup {} not found",
                    backup_id
                )));
            }
        };

        let manager = RestoreManagerFactory::create_full_restore_manager(
            self.config.clone(),
            backup,
            target_dir,
        );

        let restore = manager.restore().await?;

        // SSH tunneling is now handled by the tunnel_wrapper module
        info!("Full backup restore completed: {}", restore.id);
        Ok(restore)
    }

    /// Restore from incremental backups
    pub async fn restore_incremental_backup(
        &mut self,
        full_backup_id: &Uuid,
        target_dir: PathBuf,
    ) -> Result<Restore, PostgresError> {
        info!(
            "Starting restore from incremental backups based on full backup: {}",
            full_backup_id
        );

        // Find the full backup
        let full_backup = match self.catalog.get_backup(full_backup_id) {
            Some(backup) => {
                if backup.backup_type != BackupType::Full {
                    return Err(PostgresError::RestoreError(format!(
                        "Backup {} is not a full backup",
                        full_backup_id
                    )));
                }
                backup.clone()
            }
            None => {
                return Err(PostgresError::RestoreError(format!(
                    "Backup {} not found",
                    full_backup_id
                )));
            }
        };

        // Find all incremental backups based on this full backup
        let incremental_backups = self
            .catalog
            .get_incremental_backups_since(full_backup_id)
            .into_iter()
            .cloned()
            .collect();

        let manager = RestoreManagerFactory::create_incremental_restore_manager(
            self.config.clone(),
            full_backup,
            incremental_backups,
            target_dir,
        );

        let restore = manager.restore().await?;

        // SSH tunneling is now handled by the tunnel_wrapper module
        info!("Incremental backup restore completed: {}", restore.id);
        Ok(restore)
    }

    /// Restore to a point in time
    pub async fn restore_point_in_time(
        &mut self,
        full_backup_id: &Uuid,
        target_dir: PathBuf,
        target_time: DateTime<Utc>,
    ) -> Result<Restore, PostgresError> {
        info!(
            "Starting point-in-time restore to {} based on full backup: {}",
            target_time, full_backup_id
        );

        // Find the full backup
        let full_backup = match self.catalog.get_backup(full_backup_id) {
            Some(backup) => {
                if backup.backup_type != BackupType::Full {
                    return Err(PostgresError::RestoreError(format!(
                        "Backup {} is not a full backup",
                        full_backup_id
                    )));
                }
                backup.clone()
            }
            None => {
                return Err(PostgresError::RestoreError(format!(
                    "Backup {} not found",
                    full_backup_id
                )));
            }
        };

        // Find all incremental backups based on this full backup
        let incremental_backups = self
            .catalog
            .get_incremental_backups_since(full_backup_id)
            .into_iter()
            .cloned()
            .collect();

        let manager = RestoreManagerFactory::create_point_in_time_restore_manager(
            self.config.clone(),
            full_backup,
            incremental_backups,
            target_dir,
            target_time,
        );

        let restore = manager.restore().await?;

        // SSH tunneling is now handled by the tunnel_wrapper module

        info!("Point-in-time restore completed: {}", restore.id);
        Ok(restore)
    }

    /// Restore from a snapshot backup
    pub async fn restore_snapshot_backup(
        &mut self,
        backup_id: &Uuid,
        target_dir: PathBuf,
    ) -> Result<Restore, PostgresError> {
        info!("Restoring snapshot backup: {}", backup_id);

        let backup = self
            .catalog
            .get_backup(backup_id)
            .ok_or_else(|| PostgresError::BackupNotFound(*backup_id))?
            .clone();

        let manager = RestoreManagerFactory::create_snapshot_restore_manager(
            self.config.clone(),
            backup,
            target_dir,
        )?;

        let restore = manager.restore().await?;

        // SSH tunneling is now handled by the tunnel_wrapper module

        info!("Snapshot backup restore completed: {}", restore.id);
        Ok(restore)
    }

    /// List contents of a snapshot backup
    pub async fn list_snapshot_contents(&self, backup_id: &Uuid) -> Result<String, PostgresError> {
        info!("Listing contents of snapshot backup: {}", backup_id);

        let backup = self
            .catalog
            .get_backup(backup_id)
            .ok_or_else(|| PostgresError::BackupNotFound(*backup_id))?
            .clone();

        let manager = RestoreManagerFactory::create_snapshot_restore_manager(
            self.config.clone(),
            backup,
            PathBuf::new(), // Target dir not needed for listing contents
        )?;

        let contents = manager.list_contents().await?;

        Ok(contents)
    }

    /// List all backups
    pub fn list_backups(&self) -> &[Backup] {
        &self.catalog.backups
    }

    /// Get a specific backup
    pub fn get_backup(&self, id: &Uuid) -> Option<&Backup> {
        self.catalog.get_backup(id)
    }

    /// Get the latest full backup
    pub fn get_latest_full_backup(&self) -> Option<&Backup> {
        self.catalog.get_latest_full_backup()
    }

    /// Save the backup catalog
    fn save_catalog(&self) -> Result<(), PostgresError> {
        info!("Saving backup catalog to {}", self.catalog_path.display());
        self.catalog
            .save_to_file(&self.catalog_path)
            .map_err(PostgresError::Io)?;
        Ok(())
    }

    /// Add a backup to the catalog and save it
    pub fn add_backup_to_catalog(&mut self, backup: Backup) -> Result<(), PostgresError> {
        info!("Adding backup {} to catalog", backup.id);
        self.catalog.add_backup(backup);
        self.save_catalog()
    }
}
