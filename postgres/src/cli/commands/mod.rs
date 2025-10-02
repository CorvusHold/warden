use anyhow::{anyhow, Result};
use log::{error, info};
use std::{path::PathBuf, sync::atomic::Ordering};
use uuid::Uuid;

// Import storage module
use storage::{Metadata, PostgresBackupStorage, StorageProviderType};

use crate::common::PostgresConfig;
use crate::manager::PostgresManager;
use crate::tunnel_keeper::TunnelKeeper;
use crate::PostgresError;

mod restore_full_incremental;
pub use restore_full_incremental::{restore_full, restore_incremental};

// Helper function to create a storage provider

#[allow(clippy::too_many_arguments)]
pub async fn snapshot_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    info!("[CLI] Entering snapshot_backup");
    info!(
        "[CLI] Params: host={}, port={}, database={}, user={}, backup_dir={:?}, remote_storage={}",
        host, port, database, user, backup_dir, storage.remote_storage
    );
    let config = PostgresConfig {
        host: if ssh.host.is_some() {
            "localhost".to_string()
        } else {
            host
        },
        port: if ssh.host.is_some() {
            ssh.local_port.unwrap_or(6969)
        } else {
            port
        },
        database: database.clone(),
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let config_clone = config.clone();
    // Setup SSH tunnel if needed
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let mut keeper = keeper_instance.lock().await;
        if let Err(e) = keeper.setup(&config_clone).await {
            error!("[CLI] Failed to setup SSH tunnel: {}", e);
            return Err(anyhow!("Failed to setup SSH tunnel: {}", e));
        }
    }
    let mut manager = PostgresManager::new(config_clone.clone(), backup_dir.clone())?;
    info!("[CLI] Performing snapshot backup...");
    let backup_result = manager.snapshot_backup().await;
    let backup = backup_result.as_ref().map_err(|e| anyhow!(e.to_string()))?;
    info!("[CLI] Snapshot backup completed: {}", backup.id);
    if storage.remote_storage {
        info!("[CLI] Uploading snapshot backup to remote storage...");
        let storage_instance = create_storage_provider(&storage).await?;
        if let Some(storage) = storage_instance {
            let mut metadata = Metadata::new();
            metadata.insert("backup_id".to_string(), backup.id.to_string());
            metadata.insert(
                "backup_type".to_string(),
                format!("{:?}", backup.backup_type),
            );
            metadata.insert("database".to_string(), database.clone());
            metadata.insert("start_time".to_string(), backup.start_time.to_string());
            // Find the actual backup directory (timestamp format)
            let mut actual_backup_path = PathBuf::new();
            if let Ok(entries) = std::fs::read_dir(&backup_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .contains("snapshot_backup_")
                    {
                        actual_backup_path = path;
                        break;
                    }
                }
            }
            info!(
                "[CLI] Using backup directory: {}",
                actual_backup_path.display()
            );
            storage
                .upload_physical_backup(
                    &backup.id.to_string(),
                    &actual_backup_path,
                    Some(metadata.clone()),
                )
                .await
                .map_err(|e| anyhow!("Failed to upload physical backup: {}", e))?;
            let dump_file = actual_backup_path.join(format!("{}.dump", database));
            if dump_file.exists() {
                info!(
                    "[CLI] Uploading logical backup from: {}",
                    dump_file.display()
                );
                storage
                    .upload_logical_backup(
                        &backup.id.to_string(),
                        &dump_file,
                        Some(metadata.clone()),
                    )
                    .await
                    .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
            } else {
                info!(
                    "[CLI] Logical backup file not found at: {}",
                    dump_file.display()
                );
                let alt_dump_file = actual_backup_path.join("pg_dump.dump");
                if alt_dump_file.exists() {
                    info!(
                        "[CLI] Uploading logical backup from alternative location: {}",
                        alt_dump_file.display()
                    );
                    storage
                        .upload_logical_backup(
                            &backup.id.to_string(),
                            &alt_dump_file,
                            Some(metadata),
                        )
                        .await
                        .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
                } else {
                    info!("[CLI] No logical backup file found to upload");
                }
            }
            info!("[CLI] Snapshot backup successfully uploaded to remote storage");
        }
    }
    // Close SSH tunnel after all operations
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let is_active = {
            let keeper = keeper_instance.lock().await;
            keeper.is_active.load(std::sync::atomic::Ordering::SeqCst)
        };
        if is_active {
            let mut keeper = keeper_instance.lock().await;
            if let Err(e) = keeper.close().await {
                error!("[CLI] Warning: Error closing SSH tunnel: {}", e);
            }
        }
    }
    info!("[CLI] Exiting snapshot_backup");
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct SshOptions {
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub local_port: Option<u16>,
    pub remote_port: Option<u16>,
}

#[derive(Clone, Debug, Default)]
pub struct StorageOptions {
    pub remote_storage: bool,
    pub provider_type: Option<String>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

async fn create_storage_provider(
    storage: &StorageOptions,
) -> Result<Option<PostgresBackupStorage>> {
    if !storage.remote_storage {
        return Ok(None);
    }

    // Validate required parameters
    let bucket = storage
        .bucket
        .clone()
        .ok_or_else(|| anyhow!("Storage bucket name is required for remote storage"))?;

    // Parse provider type (default to S3)
    let provider_type = match &storage.provider_type {
        Some(provider) => match provider.to_lowercase().as_str() {
            "s3" => StorageProviderType::S3,
            _ => return Err(anyhow!("Unsupported storage provider type: {}", provider)),
        },
        None => StorageProviderType::S3,
    };

    // Create storage provider
    let storage_instance = PostgresBackupStorage::new(
        provider_type,
        bucket,
        storage.prefix.clone(),
        storage.region.clone(),
        storage.endpoint.clone(),
        storage.access_key.clone(),
        storage.secret_key.clone(),
        None, // account_id
        None, // project_id
        None, // credentials_path
    )
    .await
    .map_err(|e| anyhow!("Failed to create storage provider: {}", e))?;

    Ok(Some(storage_instance))
}

#[allow(clippy::too_many_arguments)]
pub async fn full_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    let config = PostgresConfig {
        host: if ssh.host.is_some() {
            "localhost".to_string()
        } else {
            host
        },
        port: if ssh.host.is_some() {
            ssh.local_port.unwrap_or(6969)
        } else {
            port
        },
        database: database.clone(),
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let config_clone = config.clone();
    // Setup SSH tunnel if needed
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let mut keeper = keeper_instance.lock().await;
        if let Err(e) = keeper.setup(&config_clone).await {
            return Err(anyhow!("Failed to setup SSH tunnel: {}", e));
        }
    }
    let mut manager = PostgresManager::new(config_clone.clone(), backup_dir.clone())?;
    log::info!("Performing full backup...");
    let backup_result = manager.full_backup().await;
    let backup = backup_result.as_ref().map_err(|e| anyhow!(e.to_string()))?;
    log::info!("Full backup completed: {}", backup.id);
    if storage.remote_storage {
        log::info!("Uploading full backup to remote storage...");
        let storage_instance = create_storage_provider(&storage).await?;
        if let Some(storage) = storage_instance {
            let mut metadata = Metadata::new();
            metadata.insert("backup_id".to_string(), backup.id.to_string());
            metadata.insert(
                "backup_type".to_string(),
                format!("{:?}", backup.backup_type),
            );
            metadata.insert("database".to_string(), database.clone());
            metadata.insert("start_time".to_string(), backup.start_time.to_string());
            // Find the actual backup directory (timestamp format)
            let mut actual_backup_path = PathBuf::new();
            if let Ok(entries) = std::fs::read_dir(&backup_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .contains("full_backup_")
                    {
                        actual_backup_path = path;
                        break;
                    }
                }
            }
            log::info!("Using backup directory: {}", actual_backup_path.display());
            storage
                .upload_physical_backup(
                    &backup.id.to_string(),
                    &actual_backup_path,
                    Some(metadata.clone()),
                )
                .await
                .map_err(|e| anyhow!("Failed to upload physical backup: {}", e))?;
            let dump_file = actual_backup_path.join(format!("{}.dump", database));
            if dump_file.exists() {
                log::info!("Uploading logical backup from: {}", dump_file.display());
                storage
                    .upload_logical_backup(
                        &backup.id.to_string(),
                        &dump_file,
                        Some(metadata.clone()),
                    )
                    .await
                    .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
            } else {
                log::info!("Logical backup file not found at: {}", dump_file.display());
                let alt_dump_file = actual_backup_path.join("pg_dump.dump");
                if alt_dump_file.exists() {
                    log::info!(
                        "Uploading logical backup from alternative location: {}",
                        alt_dump_file.display()
                    );
                    storage
                        .upload_logical_backup(
                            &backup.id.to_string(),
                            &alt_dump_file,
                            Some(metadata),
                        )
                        .await
                        .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
                } else {
                    log::info!("No logical backup file found to upload");
                }
            }
            log::info!("Full backup successfully uploaded to remote storage");
        }
    }
    // Close SSH tunnel after all operations
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let is_active = {
            let keeper = keeper_instance.lock().await;
            keeper.is_active.load(std::sync::atomic::Ordering::SeqCst)
        };
        if is_active {
            let mut keeper = keeper_instance.lock().await;
            if let Err(e) = keeper.close().await {
                log::error!("Warning: Error closing SSH tunnel: {}", e);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn incremental_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    let config = PostgresConfig {
        host: if ssh.host.is_some() {
            "localhost".to_string()
        } else {
            host
        },
        port: if ssh.host.is_some() {
            ssh.local_port.unwrap_or(6969)
        } else {
            port
        },
        database: database.clone(),
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let config_clone = config.clone();
    // Setup SSH tunnel if needed
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let mut keeper = keeper_instance.lock().await;
        if let Err(e) = keeper.setup(&config_clone).await {
            return Err(anyhow!("Failed to setup SSH tunnel: {}", e));
        }
    }
    let mut manager = PostgresManager::new(config_clone.clone(), backup_dir.clone())?;
    info!("Performing incremental backup...");
    let backup_result = manager.incremental_backup().await;
    let backup = backup_result.as_ref().map_err(|e| anyhow!(e.to_string()))?;
    info!("Incremental backup completed: {}", backup.id);
    if storage.remote_storage {
        info!("Uploading incremental backup to remote storage...");
        let storage_instance = create_storage_provider(&storage).await?;
        if let Some(storage) = storage_instance {
            let mut metadata = Metadata::new();
            metadata.insert("backup_id".to_string(), backup.id.to_string());
            metadata.insert(
                "backup_type".to_string(),
                format!("{:?}", backup.backup_type),
            );
            metadata.insert("database".to_string(), database.clone());
            metadata.insert("start_time".to_string(), backup.start_time.to_string());
            // Find the actual backup directory (timestamp format)
            let mut actual_backup_path = PathBuf::new();
            if let Ok(entries) = std::fs::read_dir(&backup_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .contains("incremental_backup_")
                    {
                        actual_backup_path = path;
                        break;
                    }
                }
            }
            info!("Using backup directory: {}", actual_backup_path.display());
            storage
                .upload_physical_backup(
                    &backup.id.to_string(),
                    &actual_backup_path,
                    Some(metadata.clone()),
                )
                .await
                .map_err(|e| anyhow!("Failed to upload physical backup: {}", e))?;
            let dump_file = actual_backup_path.join(format!("{}.dump", database));
            if dump_file.exists() {
                info!("Uploading logical backup from: {}", dump_file.display());
                storage
                    .upload_logical_backup(&backup.id.to_string(), &dump_file, Some(metadata))
                    .await
                    .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
            } else {
                info!("Logical backup file not found at: {}", dump_file.display());
                let alt_dump_file = actual_backup_path.join("pg_dump.dump");
                if alt_dump_file.exists() {
                    info!(
                        "Uploading logical backup from alternative location: {}",
                        alt_dump_file.display()
                    );
                    storage
                        .upload_logical_backup(
                            &backup.id.to_string(),
                            &alt_dump_file,
                            Some(metadata),
                        )
                        .await
                        .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
                } else {
                    info!("No logical backup file found to upload");
                }
            }
            info!("Incremental backup successfully uploaded to remote storage");
        }
    }
    // Close SSH tunnel after all operations
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let is_active = {
            let keeper = keeper_instance.lock().await;
            keeper.is_active.load(Ordering::SeqCst)
        };
        if is_active {
            let mut keeper = keeper_instance.lock().await;
            if let Err(e) = keeper.close().await {
                error!("Warning: Error closing SSH tunnel: {}", e);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn list_backups(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    // If listing from remote storage, fetch the backup list from there
    if storage.remote_storage {
        info!("Listing backups from remote storage...");

        // Create storage provider
        let storage_instance = create_storage_provider(&storage).await?;

        if let Some(storage) = storage_instance {
            // List all backups from the remote storage with detailed metadata
            let backups = storage
                .list_remote_backups_detailed()
                .await
                .map_err(|e| anyhow!("Failed to list backups from remote storage: {}", e))?;

            println!("\n=== Remote Backups ({}) ===", backups.len());
            
            let mut total_size = 0u64;
            for backup in &backups {
                total_size += backup.size_bytes;
                
                println!("\n📦 Backup: {}", backup.id);
                println!("   Type: {:?}", backup.backup_type);
                println!("   Status: {:?}", backup.status);
                println!("   Created: {}", backup.start_time);
                if let Some(end_time) = backup.end_time {
                    let duration = end_time - backup.start_time;
                    println!("   Duration: {}s", duration.num_seconds());
                }
                println!("   Size: {:.2} GB", backup.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0);
                println!("   Server: {}", backup.server_version);
                
                if let Some(base_id) = &backup.base_backup_id {
                    println!("   Base Backup: {}", base_id);
                }
                if backup.pinned {
                    println!("   📌 PINNED");
                }
                if !backup.tags.is_empty() {
                    println!("   Tags: {}", backup.tags.join(", "));
                }
                println!("   Files: {} files", backup.files.len());
            }
            
            println!("\n=== Summary ===");
            println!("Total backups: {}", backups.len());
            println!("Total size: {:.2} GB", total_size as f64 / 1024.0 / 1024.0 / 1024.0);

            return Ok(());
        }
    }

    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    info!("All backups:");
    for backup in manager.list_backups() {
        info!(
            "Backup ID: {}, Type: {:?}, Status: {:?}, Time: {}",
            backup.id, backup.backup_type, backup.status, backup.start_time
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn restore(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    full_backup_id: String,
    target_dir: PathBuf,
    container_id: Option<String>,
    container_type: Option<String>,
    auto_restart: bool,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if storage.remote_storage {
        info!("Downloading snapshot backup from remote storage...");

        // Create storage provider
        let storage_instance = create_storage_provider(&storage).await?;

        if let Some(storage) = storage_instance {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&full_backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }

            // Download the full backup
            storage
                .download_backup(&full_backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;

            info!("Full backup downloaded successfully");

            // Now we need to find and download all incremental backups
            // List all backups that have this full backup as ancestor
            let incremental_backups = storage
                .list_backups_with_ancestor(&full_backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;

            // Download each incremental backup
            for backup_id in incremental_backups {
                info!("Downloading incremental backup {}...", backup_id);

                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }

                storage
                    .download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }

            info!("All incremental backups downloaded successfully");
        }
    }

    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    info!(
        "Restoring with incremental backups from {} to {:?}...",
        full_backup_id, target_dir
    );
    let full_backup_id =
        Uuid::parse_str(&full_backup_id).map_err(|e: uuid::Error| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_incremental_backup(&full_backup_id, target_dir)
        .await
        .map_err(|e: PostgresError| anyhow::anyhow!(e))?;
    info!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn restore_point_in_time(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    full_backup_id: String,
    target_dir: PathBuf,
    target_time: String,
    container_id: Option<String>,
    container_type: Option<String>,
    auto_restart: bool,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if storage.remote_storage {
        info!("Downloading full and incremental backups for point-in-time restore from remote storage...");
        let storage_instance = create_storage_provider(&storage).await?;
        if let Some(storage) = storage_instance {
            let full_backup_path = backup_dir.join(&full_backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }
            storage
                .download_backup(&full_backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;
            info!("Full backup downloaded successfully");
            let incremental_backups = storage
                .list_backups_with_ancestor(&full_backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;
            for backup_id in incremental_backups {
                info!("Downloading incremental backup {}...", backup_id);
                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }
                storage
                    .download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }
        }
    }

    let config = PostgresConfig {
        host: if ssh.host.is_some() {
            "localhost".to_string()
        } else {
            host
        },
        port: if ssh.host.is_some() {
            ssh.local_port.unwrap_or(6969)
        } else {
            port
        },
        database: database.clone(),
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };

    // Setup SSH tunnel if needed
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let mut keeper = keeper_instance.lock().await;
        if let Err(e) = keeper.setup(&config).await {
            return Err(anyhow!("Failed to setup SSH tunnel: {}", e));
        }
    }

    let mut manager = PostgresManager::new(config.clone(), backup_dir.clone())?;
    // Parse target time
    let target_time = chrono::DateTime::parse_from_str(&target_time, "%Y-%m-%dT%H:%M:%S%z")
        .map_err(|e| anyhow::anyhow!("Invalid target time format: {}", e))?
        .with_timezone(&chrono::Utc);
    info!(
        "Restoring to point in time {} from {} to {:?}...",
        target_time, full_backup_id, target_dir
    );
    let full_backup_id = Uuid::parse_str(&full_backup_id).map_err(|e: uuid::Error| anyhow!(e))?;
    let restore = manager
        .restore_point_in_time(&full_backup_id, target_dir, target_time)
        .await
        .map_err(|e: PostgresError| anyhow!(e))?;
    info!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    // Close SSH tunnel after all operations (if opened)
    if config.ssh_host.is_some() {
        let keeper_instance = TunnelKeeper::instance().await;
        let is_active = {
            let keeper = keeper_instance.lock().await;
            keeper.is_active.load(std::sync::atomic::Ordering::SeqCst)
        };
        if is_active {
            let mut keeper = keeper_instance.lock().await;
            if let Err(e) = keeper.close().await {
                error!("Warning: Error closing SSH tunnel: {}", e);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn restore_snapshot(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    backup_id: String,
    target_dir: PathBuf,
    container_id: Option<String>,
    container_type: Option<String>,
    auto_restart: bool,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if storage.remote_storage {
        info!("Downloading snapshot backup from remote storage...");

        // Create storage provider
        let storage_instance = create_storage_provider(&storage).await?;

        if let Some(storage) = storage_instance {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }

            // Download the full backup
            storage
                .download_backup(&backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;

            info!("Full backup downloaded successfully");
        }
    }

    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    info!(
        "Restoring from snapshot backup {} to {:?}...",
        backup_id, target_dir
    );
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e: uuid::Error| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_snapshot_backup(&backup_id, target_dir)
        .await
        .map_err(|e: PostgresError| anyhow::anyhow!(e))?;
    info!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

/// Restart PostgreSQL after a restore operation in container or local environments
async fn restart_postgresql(
    container_id: Option<String>,
    container_type: Option<String>,
) -> Result<()> {
    match (container_id, container_type.as_deref()) {
        (Some(id), Some("docker")) => {
            info!("Restarting PostgreSQL in Docker container {}...", id);
            // Execute Docker command to restart PostgreSQL
            let output = std::process::Command::new("docker")
                .args([
                    "exec",
                    &id,
                    "pg_ctl",
                    "restart",
                    "-D",
                    "/var/lib/postgresql/data",
                ])
                .output()
                .map_err(|e| {
                    anyhow::anyhow!("Failed to restart PostgreSQL in Docker container: {}", e)
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "Failed to restart PostgreSQL in Docker container: {}",
                    error
                ));
            }

            info!("PostgreSQL successfully restarted in Docker container");
        }
        (Some(id), Some("kubernetes")) => {
            info!("Restarting PostgreSQL in Kubernetes pod {}...", id);
            // Execute kubectl command to restart PostgreSQL
            let output = std::process::Command::new("kubectl")
                .args([
                    "exec",
                    &id,
                    "--",
                    "pg_ctl",
                    "restart",
                    "-D",
                    "/var/lib/postgresql/data",
                ])
                .output()
                .map_err(|e| {
                    anyhow::anyhow!("Failed to restart PostgreSQL in Kubernetes pod: {}", e)
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "Failed to restart PostgreSQL in Kubernetes pod: {}",
                    error
                ));
            }

            info!("PostgreSQL successfully restarted in Kubernetes pod");
        }
        (Some(_), Some(invalid_type)) => {
            return Err(anyhow::anyhow!(
                "Invalid container type: {}. Supported types are 'docker' and 'kubernetes'",
                invalid_type
            ));
        }
        (Some(_), None) => {
            return Err(anyhow::anyhow!("Container ID provided but container type is missing. Please specify --container-type"));
        }
        (None, Some(_)) => {
            return Err(anyhow::anyhow!("Container type provided but container ID is missing. Please specify --container-id"));
        }
        (None, None) => {
            // Attempt to restart local PostgreSQL instance
            info!("Attempting to restart local PostgreSQL instance...");

            // Detect operating system
            let os = std::env::consts::OS;
            match os {
                "macos" => restart_postgresql_macos().await?,
                "linux" => restart_postgresql_linux().await?,
                _ => {
                    info!("Auto-restart not supported on {} operating system. Please restart PostgreSQL manually.", os);
                }
            }
        }
    }

    Ok(())
}

/// Restart PostgreSQL on macOS
async fn restart_postgresql_macos() -> Result<()> {
    // Try different methods for restarting PostgreSQL on macOS

    // Method 1: Using brew services (most common for Homebrew installations)
    if let Ok(output) = std::process::Command::new("brew")
        .args(["services", "restart", "postgresql"])
        .output()
    {
        if output.status.success() {
            info!("PostgreSQL successfully restarted using Homebrew services");
            return Ok(());
        }
    }

    // Method 2: Using pg_ctl directly (try common data directories)
    let data_dirs = [
        "/usr/local/var/postgres",
        "/opt/homebrew/var/postgres",
        "/usr/local/var/postgresql@14", // For specific versions
        "/usr/local/var/postgresql@13",
        "/usr/local/var/postgresql@12",
    ];

    for data_dir in data_dirs {
        if std::path::Path::new(data_dir).exists() {
            if let Ok(output) = std::process::Command::new("pg_ctl")
                .args(["restart", "-D", data_dir])
                .output()
            {
                if output.status.success() {
                    info!(
                        "PostgreSQL successfully restarted using pg_ctl with data directory: {}",
                        data_dir
                    );
                    return Ok(());
                }
            }
        }
    }

    // Method 3: Using launchctl for system installations
    if let Ok(output) = std::process::Command::new("launchctl")
        .args([
            "unload",
            "/Library/LaunchDaemons/org.postgresql.postgres.plist",
        ])
        .output()
    {
        if output.status.success() {
            if let Ok(output) = std::process::Command::new("launchctl")
                .args([
                    "load",
                    "/Library/LaunchDaemons/org.postgresql.postgres.plist",
                ])
                .output()
            {
                if output.status.success() {
                    info!("PostgreSQL successfully restarted using launchctl");
                    return Ok(());
                }
            }
        }
    }

    info!("Could not automatically restart PostgreSQL on macOS. Please restart it manually.");
    Ok(())
}

/// Restart PostgreSQL on Linux
async fn restart_postgresql_linux() -> Result<()> {
    // Try different methods for restarting PostgreSQL on Linux

    // Method 1: Using systemctl (most common on modern distros)
    if let Ok(output) = std::process::Command::new("systemctl")
        .args(["restart", "postgresql"])
        .output()
    {
        if output.status.success() {
            info!("PostgreSQL successfully restarted using systemctl");
            return Ok(());
        }
    }

    // Method 2: Try with specific version numbers
    for version in ["14", "13", "12", "11", "10", "9.6"] {
        let service_name = format!("postgresql-{}", version);
        if let Ok(output) = std::process::Command::new("systemctl")
            .args(["restart", &service_name])
            .output()
        {
            if output.status.success() {
                info!(
                    "PostgreSQL {} successfully restarted using systemctl",
                    version
                );
                return Ok(());
            }
        }
    }

    // Method 3: Using service command (older distros)
    if let Ok(output) = std::process::Command::new("service")
        .args(["postgresql", "restart"])
        .output()
    {
        if output.status.success() {
            info!("PostgreSQL successfully restarted using service command");
            return Ok(());
        }
    }

    // Method 4: Using pg_ctl directly with common data directories
    let data_dirs = [
        "/var/lib/postgresql/data",
        "/var/lib/postgresql/14/data",
        "/var/lib/postgresql/13/data",
        "/var/lib/postgresql/12/data",
    ];

    for data_dir in data_dirs {
        if std::path::Path::new(data_dir).exists() {
            if let Ok(output) = std::process::Command::new("pg_ctl")
                .args(["restart", "-D", data_dir])
                .output()
            {
                if output.status.success() {
                    info!(
                        "PostgreSQL successfully restarted using pg_ctl with data directory: {}",
                        data_dir
                    );
                    return Ok(());
                }
            }
        }
    }

    info!("Could not automatically restart PostgreSQL on Linux. Please restart it manually.");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn list_snapshot_contents(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    backup_id: String,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if storage.remote_storage {
        info!("Downloading incremental backups from remote storage...");

        // Create storage provider
        let storage_instance = create_storage_provider(&storage).await?;

        if let Some(storage) = storage_instance {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }

            // Download the full backup
            storage
                .download_backup(&backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;

            info!("Full backup downloaded successfully");

            // Now we need to find and download all incremental backups
            // List all backups that have this full backup as ancestor
            let incremental_backups = storage
                .list_backups_with_ancestor(&backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;

            // Download each incremental backup
            for backup_id in incremental_backups {
                info!("Downloading incremental backup {}...", backup_id);

                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }

                storage
                    .download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }

            info!("All incremental backups downloaded successfully");
        }
    }

    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    info!("Snapshot backup contents for {}:", backup_id);
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e: uuid::Error| anyhow::anyhow!(e))?;
    let contents = manager
        .list_snapshot_contents(&backup_id)
        .await
        .map_err(|e: PostgresError| anyhow::anyhow!(e))?;
    for item in contents.split('\n').filter(|s| !s.is_empty()) {
        info!("{}", item);
    }
    Ok(())
}

/// Inspect detailed backup metadata from remote storage
pub async fn inspect_backup(storage: StorageOptions, backup_id: String) -> Result<()> {
    info!("Inspecting backup {} from remote storage...", backup_id);

    let storage_instance = create_storage_provider(&storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    let metadata = storage_instance
        .get_remote_backup_metadata(&backup_id)
        .await
        .map_err(|e| anyhow!("Failed to get backup metadata: {}", e))?;

    // Display metadata in a readable format
    println!("\n=== Backup Metadata ===");
    println!("ID: {}", metadata.id);
    println!("Type: {:?}", metadata.backup_type);
    println!("Status: {:?}", metadata.status);
    println!("Start Time: {}", metadata.start_time);
    if let Some(end_time) = metadata.end_time {
        println!("End Time: {}", end_time);
        let duration = end_time - metadata.start_time;
        println!("Duration: {} seconds", duration.num_seconds());
    }
    println!("Size: {} bytes ({:.2} GB)", metadata.size_bytes, metadata.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0);
    println!("Server Version: {}", metadata.server_version);
    
    if let Some(base_id) = &metadata.base_backup_id {
        println!("Base Backup ID: {}", base_id);
    }
    if let Some(wal_start) = &metadata.wal_start {
        println!("WAL Start: {}", wal_start);
    }
    if let Some(wal_end) = &metadata.wal_end {
        println!("WAL End: {}", wal_end);
    }
    if let Some(checksum) = &metadata.checksum {
        println!("Checksum: {}", checksum);
    }
    
    println!("Pinned: {}", metadata.pinned);
    if !metadata.tags.is_empty() {
        println!("Tags: {}", metadata.tags.join(", "));
    }
    
    println!("\n=== Files ({}) ===", metadata.files.len());
    for file in &metadata.files {
        println!("  {} ({} bytes)", file.name, file.size);
        if let Some(checksum) = &file.checksum {
            println!("    Checksum: {}", checksum);
        }
    }

    Ok(())
}

/// Download backup from remote storage
pub async fn download_backup(
    storage: StorageOptions,
    backup_id: String,
    target_dir: PathBuf,
    _verify_checksums: bool,
) -> Result<()> {
    info!("Downloading backup {} to {:?}...", backup_id, target_dir);

    let storage_instance = create_storage_provider(&storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    // Create target directory
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| anyhow!("Failed to create target directory: {}", e))?;

    // Download the backup
    storage_instance
        .download_backup(&backup_id, &target_dir)
        .await
        .map_err(|e| anyhow!("Failed to download backup: {}", e))?;

    info!("Backup {} downloaded successfully to {:?}", backup_id, target_dir);

    Ok(())
}

/// Initialize or update retention policy
pub async fn init_retention_policy(
    storage: StorageOptions,
    policy_file: PathBuf,
) -> Result<()> {
    info!("Initializing retention policy from {:?}...", policy_file);

    // Read and parse the policy file
    let policy_json = std::fs::read_to_string(&policy_file)
        .map_err(|e| anyhow!("Failed to read policy file: {}", e))?;

    let policy: storage::RetentionPolicy = serde_json::from_str(&policy_json)
        .map_err(|e| anyhow!("Failed to parse policy file: {}", e))?;

    // Validate policy
    info!("Policy version: {}", policy.version);
    info!("Policy enabled: {}", policy.enabled);
    info!("Policy type: {:?}", policy.policy_type);

    let storage_instance = create_storage_provider(&storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    // Upload the policy
    storage_instance
        .save_retention_policy(&policy)
        .await
        .map_err(|e| anyhow!("Failed to save retention policy: {}", e))?;

    info!("Retention policy saved successfully to bucket {}", storage.bucket.unwrap_or_default());

    Ok(())
}

/// Show current retention policy
pub async fn show_retention_policy(storage: StorageOptions) -> Result<()> {
    info!("Loading retention policy...");

    let storage_instance = create_storage_provider(&storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    match storage_instance.load_retention_policy().await {
        Ok(Some(policy)) => {
            println!("\n=== Retention Policy ===");
            println!("{}", serde_json::to_string_pretty(&policy).unwrap());
        }
        Ok(None) => {
            println!("No retention policy found for this bucket.");
            println!("Use 'init-retention-policy' to create one.");
        }
        Err(e) => {
            return Err(anyhow!("Failed to load retention policy: {}", e));
        }
    }

    Ok(())
}

/// Evaluate purge policy (dry run)
pub async fn purge_plan(storage: StorageOptions, format: String) -> Result<()> {
    info!("Evaluating purge policy...");

    let storage_instance = create_storage_provider(&storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    // Load policy
    let policy = storage_instance
        .load_retention_policy()
        .await
        .map_err(|e| anyhow!("Failed to load retention policy: {}", e))?
        .ok_or_else(|| anyhow!("No retention policy found. Use 'init-retention-policy' to create one."))?;

    // Evaluate purge
    let evaluation = storage_instance
        .evaluate_purge(&policy)
        .await
        .map_err(|e| anyhow!("Failed to evaluate purge: {}", e))?;

    match format.as_str() {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&evaluation).unwrap());
        }
        "yaml" => {
            println!("{}", serde_yaml::to_string(&evaluation).unwrap());
        }
        "table" | _ => {
            println!("\n=== Purge Evaluation ===");
            println!("Timestamp: {}", evaluation.timestamp);
            println!("Total backups: {}", evaluation.total_backups);
            println!("To keep: {}", evaluation.to_keep.len());
            println!("To delete: {}", evaluation.to_delete.len());
            println!("Space to free: {} bytes ({:.2} GB)", 
                evaluation.estimated_space_freed,
                evaluation.estimated_space_freed as f64 / 1024.0 / 1024.0 / 1024.0
            );

            if !evaluation.warnings.is_empty() {
                println!("\n=== Warnings ===");
                for warning in &evaluation.warnings {
                    println!("  ⚠️  {}", warning);
                }
            }

            if !evaluation.to_delete.is_empty() {
                println!("\n=== Backups to Delete ===");
                for decision in &evaluation.to_delete {
                    println!("  🗑️  {} ({:?}) - {} - {:.2} GB",
                        decision.backup_id,
                        decision.backup_type,
                        decision.reason,
                        decision.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0
                    );
                }
            }

            if !evaluation.to_keep.is_empty() {
                println!("\n=== Backups to Keep ===");
                for decision in &evaluation.to_keep {
                    println!("  ✅ {} ({:?}) - {}",
                        decision.backup_id,
                        decision.backup_type,
                        decision.reason
                    );
                }
            }
        }
    }

    Ok(())
}

/// Execute purge according to retention policy
pub async fn purge(storage: StorageOptions, apply: bool, yes: bool) -> Result<()> {
    let storage_instance = create_storage_provider(&storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    // Load policy
    let policy = storage_instance
        .load_retention_policy()
        .await
        .map_err(|e| anyhow!("Failed to load retention policy: {}", e))?
        .ok_or_else(|| anyhow!("No retention policy found. Use 'init-retention-policy' to create one."))?;

    // Evaluate purge
    let evaluation = storage_instance
        .evaluate_purge(&policy)
        .await
        .map_err(|e| anyhow!("Failed to evaluate purge: {}", e))?;

    if !apply {
        println!("\n⚠️  DRY RUN MODE - No backups will be deleted");
        println!("Use --apply to actually execute the purge\n");
    }

    println!("=== Purge Summary ===");
    println!("Total backups: {}", evaluation.total_backups);
    println!("To delete: {}", evaluation.to_delete.len());
    println!("To keep: {}", evaluation.to_keep.len());
    println!("Space to free: {} bytes ({:.2} GB)", 
        evaluation.estimated_space_freed,
        evaluation.estimated_space_freed as f64 / 1024.0 / 1024.0 / 1024.0
    );

    if !evaluation.to_delete.is_empty() {
        println!("\nBackups to be deleted:");
        for decision in &evaluation.to_delete {
            println!("  - {} ({:?}) - {}", 
                decision.backup_id,
                decision.backup_type,
                decision.reason
            );
        }
    }

    // Confirm if applying and confirmation required
    if apply && !yes && policy.safety.require_confirmation {
        use std::io::{self, Write};
        print!("\nAre you sure you want to delete {} backups? (yes/no): ", evaluation.to_delete.len());
        io::stdout().flush().unwrap();
        
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        
        if input.trim().to_lowercase() != "yes" {
            println!("Purge cancelled.");
            return Ok(());
        }
    }

    // Execute purge
    let report = storage_instance
        .execute_purge(&evaluation, !apply)
        .await
        .map_err(|e| anyhow!("Failed to execute purge: {}", e))?;

    println!("\n=== Purge Report ===");
    println!("Dry run: {}", report.dry_run);
    println!("Total evaluated: {}", report.total_evaluated);
    println!("Kept: {}", report.kept);
    println!("Deleted: {}", report.deleted);
    println!("Failed: {}", report.failed);
    println!("Space freed: {} bytes ({:.2} GB)", 
        report.space_freed,
        report.space_freed as f64 / 1024.0 / 1024.0 / 1024.0
    );
    println!("Duration: {} seconds", report.duration_secs);

    if !report.errors.is_empty() {
        println!("\n=== Errors ===");
        for error in &report.errors {
            println!("  ❌ {}", error);
        }
    }

    // Report to Sentry
    storage::purge::report_purge_to_sentry(&report, &policy);

    if report.dry_run {
        println!("\n⚠️  This was a dry run. Use --apply to actually delete backups.");
    } else {
        println!("\n✅ Purge completed successfully.");
    }

    Ok(())
}
