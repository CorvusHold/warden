use anyhow::{Result, anyhow};
use log::{error, info};
use std::{path::PathBuf, sync::atomic::Ordering};
use uuid::Uuid;

// Import storage module
use storage::{PostgresBackupStorage, StorageProviderType, Metadata};

use crate::{PostgresConfig, PostgresManager};


// Helper function to create a storage provider
async fn create_storage_provider(
    remote_storage: bool,
    provider_type: Option<String>,
    bucket: Option<String>,
    prefix: Option<String>,
    region: Option<String>,
    endpoint: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<Option<PostgresBackupStorage>> {
    if !remote_storage {
        return Ok(None);
    }
    
    // Validate required parameters
    let bucket = bucket.ok_or_else(|| anyhow!("Storage bucket name is required for remote storage"))?;
    
    // Parse provider type (default to S3)
    let provider_type = match provider_type {
        Some(provider) => match provider.to_lowercase().as_str() {
            "s3" => StorageProviderType::S3,
            _ => return Err(anyhow!("Unsupported storage provider type: {}", provider)),
        },
        None => StorageProviderType::S3,
    };
    
    // Create storage provider
    let storage = PostgresBackupStorage::new(
        provider_type,
        bucket,
        prefix,
        region,
        endpoint,
        access_key,
        secret_key,
        None, // account_id
        None, // project_id
        None, // credentials_path
    )
    .await
    .map_err(|e| anyhow!("Failed to create storage provider: {}", e))?;
    
    Ok(Some(storage))
}

pub async fn full_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    // SSH options
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
    // Storage options
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
) -> Result<()> {
    let config = PostgresConfig {
        host: if ssh_host.is_some() {
            "localhost".to_string() // Use localhost when using SSH tunnel
        } else {
            host
        },
        port: if ssh_host.is_some() {
            ssh_local_port.unwrap_or(6969) // Use local tunnel port
        } else {
            port
        },
        database: database.clone(),
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port,
    };
    // Create a copy of the config for potential modification
    let mut config_clone = config.clone();
    
    // Setup SSH tunnel if needed using the global tunnel keeper
    if config.ssh_host.is_some() {
        // Store the instance in a variable first to avoid temporary value issues
        let keeper_instance = crate::tunnel_keeper::TunnelKeeper::instance();
        let mut keeper = keeper_instance.lock().unwrap();
        if let Err(e) = keeper.setup(&mut config_clone).await {
            return Err(anyhow!("Failed to setup SSH tunnel: {}", e));
        }
        // Drop the lock immediately after setup
        // drop(keeper);
    }
    
    // Create PostgreSQL manager with the possibly modified config
    let mut manager = PostgresManager::new(config_clone, backup_dir.clone())?;
    info!("Performing full backup...");

    // Perform the backup operation
    let backup_result = manager.full_backup().await;

    // Now handle the backup result
    let backup = backup_result.map_err(|e| anyhow!(e))?;
    info!("Full backup completed: {}", backup.id);

    // Upload to remote storage if requested
    if remote_storage {
        info!("Uploading backup to remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // Create metadata for the backup
            let mut metadata = Metadata::new();
            metadata.insert("backup_id".to_string(), backup.id.to_string());
            metadata.insert("backup_type".to_string(), format!("{:?}", backup.backup_type));
            metadata.insert("database".to_string(), database.clone());
            metadata.insert("start_time".to_string(), backup.start_time.to_string());
            
            // Find the actual backup directory (which has a timestamp format)
            let mut actual_backup_path = PathBuf::new();
            if let Ok(entries) = std::fs::read_dir(&backup_dir) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_dir() && path.file_name().unwrap_or_default().to_string_lossy().contains("full_backup_") {
                            actual_backup_path = path;
                            break;
                        }
                    }
                }
            }
            
            info!("Using backup directory: {}", actual_backup_path.display());
            
            // Upload physical backup files
            storage.upload_physical_backup(&backup.id.to_string(), &actual_backup_path, Some(metadata.clone()))
                .await
                .map_err(|e| anyhow!("Failed to upload physical backup: {}", e))?;
            
            // Upload logical backup if available
            let dump_file = actual_backup_path.join(format!("{}.dump", database));
            if dump_file.exists() {
                info!("Uploading logical backup from: {}", dump_file.display());
                storage.upload_logical_backup(&backup.id.to_string(), &dump_file, Some(metadata))
                    .await
                    .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
            } else {
                info!("Logical backup file not found at: {}", dump_file.display());
                // Try alternative locations
                let alt_dump_file = actual_backup_path.join("pg_dump.dump");
                if alt_dump_file.exists() {
                    info!("Uploading logical backup from alternative location: {}", alt_dump_file.display());
                    storage.upload_logical_backup(&backup.id.to_string(), &alt_dump_file, Some(metadata))
                        .await
                        .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
                } else {
                    info!("No logical backup file found to upload");
                }
            }
            
            info!("Backup successfully uploaded to remote storage");
        }
    }

    // Close SSH tunnel explicitly after ALL operations are complete
    if config.ssh_host.is_some() {
        let keeper_instance = crate::tunnel_keeper::TunnelKeeper::instance();
        let mut keeper = keeper_instance.lock().unwrap();
        if keeper.is_active.load(Ordering::SeqCst) {
            if let Err(e) = keeper.close().await {
                error!("Warning: Error closing SSH tunnel: {}", e);
            }
        }
    }
    
    
    // // Now handle the backup result
    // let backup = backup_result.map_err(|e| anyhow!(e))?;
    // info!("Full backup completed: {}", backup.id);
    
    // // Upload to remote storage if requested
    // if remote_storage {
    //     info!("Uploading backup to remote storage...");
        
    //     // Create storage provider
    //     let storage = create_storage_provider(
    //         remote_storage,
    //         storage_provider,
    //         storage_bucket,
    //         storage_prefix,
    //         storage_region,
    //         storage_endpoint,
    //         storage_access_key,
    //         storage_secret_key,
    //     ).await?;
        
    //     if let Some(storage) = storage {
    //         // Create metadata for the backup
    //         let mut metadata = Metadata::new();
    //         metadata.insert("backup_id".to_string(), backup.id.to_string());
    //         metadata.insert("backup_type".to_string(), format!("{:?}", backup.backup_type));
    //         metadata.insert("database".to_string(), database.clone());
    //         metadata.insert("start_time".to_string(), backup.start_time.to_string());
            
    //         // Find the actual backup directory (which has a timestamp format)
    //         let mut actual_backup_path = PathBuf::new();
    //         if let Ok(entries) = std::fs::read_dir(&backup_dir) {
    //             for entry in entries {
    //                 if let Ok(entry) = entry {
    //                     let path = entry.path();
    //                     if path.is_dir() && path.file_name().unwrap_or_default().to_string_lossy().contains("full_backup_") {
    //                         actual_backup_path = path;
    //                         break;
    //                     }
    //                 }
    //             }
    //         }
            
    //         info!("Using backup directory: {}", actual_backup_path.display());
            
    //         // Upload physical backup files
    //         storage.upload_physical_backup(&backup.id.to_string(), &actual_backup_path, Some(metadata.clone()))
    //             .await
    //             .map_err(|e| anyhow!("Failed to upload physical backup: {}", e))?;
            
    //         // Upload logical backup if available
    //         let dump_file = actual_backup_path.join(format!("{}.dump", database));
    //         if dump_file.exists() {
    //             info!("Uploading logical backup from: {}", dump_file.display());
    //             storage.upload_logical_backup(&backup.id.to_string(), &dump_file, Some(metadata))
    //                 .await
    //                 .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
    //         } else {
    //             info!("Logical backup file not found at: {}", dump_file.display());
    //             // Try alternative locations
    //             let alt_dump_file = actual_backup_path.join("pg_dump.dump");
    //             if alt_dump_file.exists() {
    //                 info!("Uploading logical backup from alternative location: {}", alt_dump_file.display());
    //                 storage.upload_logical_backup(&backup.id.to_string(), &alt_dump_file, Some(metadata))
    //                     .await
    //                     .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
    //             } else {
    //                 info!("No logical backup file found to upload");
    //             }
    //         }
            
    //         info!("Backup successfully uploaded to remote storage");
    //     }
    // }
    
    Ok(())
}

pub async fn incremental_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database: database.clone(),
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port
    };
    // Create a copy of the config for potential modification
    let mut config_clone = config.clone();
    
    // Setup SSH tunnel if needed using the global tunnel keeper
    if config.ssh_host.is_some() {
        // Store the instance in a variable first to avoid temporary value issues
        let keeper_instance = crate::tunnel_keeper::TunnelKeeper::instance();
        let mut keeper = keeper_instance.lock().unwrap();
        if let Err(e) = keeper.setup(&mut config_clone).await {
            return Err(anyhow!("Failed to setup SSH tunnel: {}", e));
        }
        // Drop the lock immediately after setup
        drop(keeper);
    }
    
    // Create PostgreSQL manager with the possibly modified config
    let mut manager = PostgresManager::new(config_clone, backup_dir.clone())?;
    println!("Performing incremental backup...");
    
    // Perform the backup operation
    let backup_result = manager.incremental_backup().await;
    
    // Close SSH tunnel explicitly after the operation is complete
    if config.ssh_host.is_some() {
        // Store the instance in a variable first to avoid temporary value issues
        let keeper_instance = crate::tunnel_keeper::TunnelKeeper::instance();
        let mut keeper = keeper_instance.lock().unwrap();
        if keeper.is_active.load(Ordering::SeqCst) {
            if let Err(e) = keeper.close().await {
                eprintln!("Warning: Error closing SSH tunnel: {}", e);
            }
        }
    }
    
    // Now handle the backup result
    let backup = backup_result.map_err(|e| anyhow!(e))?;
    println!("Incremental backup completed: {}", backup.id);
    
    // Upload to remote storage if requested
    if remote_storage {
        println!("Uploading incremental backup to remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // Create metadata for the backup
            let mut metadata = Metadata::new();
            metadata.insert("backup_id".to_string(), backup.id.to_string());
            metadata.insert("backup_type".to_string(), format!("{:?}", backup.backup_type));
            metadata.insert("database".to_string(), database.clone());
            metadata.insert("start_time".to_string(), backup.start_time.to_string());
            if let Some(base_backup_id) = backup.base_backup_id {
                metadata.insert("parent_id".to_string(), base_backup_id.to_string());
            }
            
            // Upload incremental backup files
            let backup_path = backup_dir.join(backup.id.to_string());
            storage.upload_backup(&backup.id.to_string(), &backup_path, Some(metadata))
                .await
                .map_err(|e| anyhow!("Failed to upload incremental backup: {}", e))?;
            
            println!("Incremental backup successfully uploaded to remote storage");
        }
    }
    
    Ok(())
}

pub async fn snapshot_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database: database.clone(),
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port
    };
    // Create a copy of the config for potential modification
    let mut config_clone = config.clone();
    
    // Setup SSH tunnel if needed using the global tunnel keeper
    if config.ssh_host.is_some() {
        // Store the instance in a variable first to avoid temporary value issues
        let keeper_instance = crate::tunnel_keeper::TunnelKeeper::instance();
        let mut keeper = keeper_instance.lock().unwrap();
        if let Err(e) = keeper.setup(&mut config_clone).await {
            return Err(anyhow!("Failed to setup SSH tunnel: {}", e));
        }
        // Drop the lock immediately after setup
        drop(keeper);
    }
    
    // Create PostgreSQL manager with the possibly modified config
    let mut manager = PostgresManager::new(config_clone, backup_dir.clone())?;
    println!("Performing snapshot backup...");
    
    // Perform the backup operation
    let backup_result = manager.snapshot_backup().await;
    
    // Close SSH tunnel explicitly after the operation is complete
    if config.ssh_host.is_some() {
        // Store the instance in a variable first to avoid temporary value issues
        let keeper_instance = crate::tunnel_keeper::TunnelKeeper::instance();
        let mut keeper = keeper_instance.lock().unwrap();
        if keeper.is_active.load(Ordering::SeqCst) {
            if let Err(e) = keeper.close().await {
                eprintln!("Warning: Error closing SSH tunnel: {}", e);
            }
        }
    }
    
    // Now handle the backup result
    let backup = backup_result.map_err(|e| anyhow!(e))?;
    println!("Snapshot backup completed: {}", backup.id);
    
    // Upload to remote storage if requested
    if remote_storage {
        println!("Uploading snapshot backup to remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // Create metadata for the backup
            let mut metadata = Metadata::new();
            metadata.insert("backup_id".to_string(), backup.id.to_string());
            metadata.insert("backup_type".to_string(), format!("{:?}", backup.backup_type));
            metadata.insert("database".to_string(), database.clone());
            metadata.insert("start_time".to_string(), backup.start_time.to_string());
            
            // Upload snapshot backup files
            let backup_path = backup_dir.join(backup.id.to_string());
            storage.upload_backup(&backup.id.to_string(), &backup_path, Some(metadata.clone()))
                .await
                .map_err(|e| anyhow!("Failed to upload snapshot backup: {}", e))?;
            
            // Upload logical backup if available (snapshots typically include a logical backup)
            let dump_file = backup_path.join("pg_dump.dump");
            if dump_file.exists() {
                storage.upload_logical_backup(&backup.id.to_string(), &dump_file, Some(metadata))
                    .await
                    .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;
            }
            
            println!("Snapshot backup successfully uploaded to remote storage");
        }
    }
    
    Ok(())
}

pub async fn list_backups(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    // SSH options
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
    // Storage options
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
) -> Result<()> {
    // If listing from remote storage, fetch the backup list from there
    if remote_storage {
        println!("Listing backups from remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // List all backups from the remote storage
            let backups = storage.list_backups()
                .await
                .map_err(|e| anyhow!("Failed to list backups from remote storage: {}", e))?;
            
            println!("All backups in remote storage:");
            for backup in backups {
                println!(
                    "Backup ID: {}, Type: {:?}, Time: {}",
                    backup.id, backup.backup_type, backup.timestamp
                );
            }
            
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
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    println!("All backups:");
    for backup in manager.list_backups() {
        println!(
            "Backup ID: {}, Type: {:?}, Status: {:?}, Time: {}",
            backup.id, backup.backup_type, backup.status, backup.start_time
        );
    }
    Ok(())
}

pub async fn restore_full(
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
    // SSH options
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
    // Storage options
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if remote_storage {
        println!("Downloading backup {} from remote storage...", backup_id);
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider.clone(),
            storage_bucket.clone(),
            storage_prefix.clone(),
            storage_region.clone(),
            storage_endpoint.clone(),
            storage_access_key.clone(),
            storage_secret_key.clone(),
        ).await?;
        
        if let Some(storage) = storage {
            // Create backup directory if it doesn't exist
            let backup_path = backup_dir.join(&backup_id);
            if !backup_path.exists() {
                std::fs::create_dir_all(&backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }
            
            // Download the backup
            storage.download_backup(&backup_id, &backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download backup: {}", e))?;
                
            println!("Backup downloaded successfully");
        }
    }
    
    // If restoring from remote storage, download the backup first
    if remote_storage {
        println!("Downloading incremental backups from remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider.clone(),
            storage_bucket.clone(),
            storage_prefix.clone(),
            storage_region.clone(),
            storage_endpoint.clone(),
            storage_access_key.clone(),
            storage_secret_key.clone(),
        ).await?;
        
        if let Some(storage) = storage {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }
            
            // Download the full backup
            storage.download_backup(&backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;
            
            println!("Full backup downloaded successfully");
            
            // Now we need to find and download all incremental backups
            // List all backups that have this full backup as ancestor
            let incremental_backups = storage.list_backups_with_ancestor(&backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;
            
            // Download each incremental backup
            for backup_id in incremental_backups {
                println!("Downloading incremental backup {}...", backup_id);
                
                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }
                
                storage.download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }
            
            println!("All incremental backups downloaded successfully");
        }
    }
    
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    println!(
        "Restoring from full backup {} to {:?}...",
        backup_id, target_dir
    );
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e| anyhow!(e))?;
    let restore = manager
        .restore_full_backup(&backup_id, target_dir)
        .await
        .map_err(|e| anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

pub async fn restore_incremental(
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
    // SSH options
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
    // Storage options
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if remote_storage {
        println!("Downloading incremental backups from remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&full_backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }
            
            // Download the full backup
            storage.download_backup(&full_backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;
            
            println!("Full backup downloaded successfully");
            
            // Now we need to find and download all incremental backups
            // List all backups that have this full backup as ancestor
            let incremental_backups = storage.list_backups_with_ancestor(&full_backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;
            
            // Download each incremental backup
            for backup_id in incremental_backups {
                println!("Downloading incremental backup {}...", backup_id);
                
                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }
                
                storage.download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }
            
            println!("All incremental backups downloaded successfully");
        }
    }
    
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    println!(
        "Restoring with incremental backups from {} to {:?}...",
        full_backup_id, target_dir
    );
    let full_backup_id = Uuid::parse_str(&full_backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_incremental_backup(&full_backup_id, target_dir)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

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
    // SSH options
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
    // Storage options
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if remote_storage {
        println!("Downloading incremental backups from remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&full_backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }
            
            // Download the full backup
            storage.download_backup(&full_backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;
            
            println!("Full backup downloaded successfully");
            
            // Now we need to find and download all incremental backups
            // List all backups that have this full backup as ancestor
            let incremental_backups = storage.list_backups_with_ancestor(&full_backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;
            
            // Download each incremental backup
            for backup_id in incremental_backups {
                println!("Downloading incremental backup {}...", backup_id);
                
                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }
                
                storage.download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }
            
            println!("All incremental backups downloaded successfully");
        }
    }
    
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    // Parse target time
    let target_time = chrono::DateTime::parse_from_str(&target_time, "%Y-%m-%dT%H:%M:%S%z")
        .map_err(|e| anyhow::anyhow!("Invalid target time format: {}", e))?
        .with_timezone(&chrono::Utc);

    println!(
        "Restoring to point in time {} from {} to {:?}...",
        target_time, full_backup_id, target_dir
    );
    let full_backup_id = Uuid::parse_str(&full_backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_point_in_time(&full_backup_id, target_dir, target_time)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

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
    // SSH options
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
    // Storage options
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if remote_storage {
        println!("Downloading incremental backups from remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }
            
            // Download the full backup
            storage.download_backup(&backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;
            
            println!("Full backup downloaded successfully");
            
            // Now we need to find and download all incremental backups
            // List all backups that have this full backup as ancestor
            let incremental_backups = storage.list_backups_with_ancestor(&backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;
            
            // Download each incremental backup
            for backup_id in incremental_backups {
                println!("Downloading incremental backup {}...", backup_id);
                
                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }
                
                storage.download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }
            
            println!("All incremental backups downloaded successfully");
        }
    }
    
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    println!(
        "Restoring from snapshot backup {} to {:?}...",
        backup_id, target_dir
    );
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_snapshot_backup(&backup_id, target_dir)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

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
            println!("Restarting PostgreSQL in Docker container {}...", id);
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

            println!("PostgreSQL successfully restarted in Docker container");
        }
        (Some(id), Some("kubernetes")) => {
            println!("Restarting PostgreSQL in Kubernetes pod {}...", id);
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

            println!("PostgreSQL successfully restarted in Kubernetes pod");
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
            println!("Attempting to restart local PostgreSQL instance...");

            // Detect operating system
            let os = std::env::consts::OS;
            match os {
                "macos" => restart_postgresql_macos().await?,
                "linux" => restart_postgresql_linux().await?,
                _ => {
                    println!("Auto-restart not supported on {} operating system. Please restart PostgreSQL manually.", os);
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
            println!("PostgreSQL successfully restarted using Homebrew services");
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
                    println!(
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
                    println!("PostgreSQL successfully restarted using launchctl");
                    return Ok(());
                }
            }
        }
    }

    println!("Could not automatically restart PostgreSQL on macOS. Please restart it manually.");
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
            println!("PostgreSQL successfully restarted using systemctl");
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
                println!(
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
            println!("PostgreSQL successfully restarted using service command");
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
                    println!(
                        "PostgreSQL successfully restarted using pg_ctl with data directory: {}",
                        data_dir
                    );
                    return Ok(());
                }
            }
        }
    }

    println!("Could not automatically restart PostgreSQL on Linux. Please restart it manually.");
    Ok(())
}

pub async fn list_snapshot_contents(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    backup_id: String,
    // SSH options
    ssh_host: Option<String>,
    ssh_user: Option<String>,
    ssh_port: Option<u16>,
    ssh_password: Option<String>,
    ssh_key_path: Option<String>,
    ssh_local_port: Option<u16>,
    ssh_remote_port: Option<u16>,
    // Storage options
    remote_storage: bool,
    storage_provider: Option<String>,
    storage_bucket: Option<String>,
    storage_prefix: Option<String>,
    storage_region: Option<String>,
    storage_endpoint: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if remote_storage {
        println!("Downloading incremental backups from remote storage...");
        
        // Create storage provider
        let storage = create_storage_provider(
            remote_storage,
            storage_provider,
            storage_bucket,
            storage_prefix,
            storage_region,
            storage_endpoint,
            storage_access_key,
            storage_secret_key,
        ).await?;
        
        if let Some(storage) = storage {
            // Create backup directory if it doesn't exist
            let full_backup_path = backup_dir.join(&backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path)
                    .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
            }
            
            // Download the full backup
            storage.download_backup(&backup_id, &full_backup_path)
                .await
                .map_err(|e| anyhow!("Failed to download full backup: {}", e))?;
            
            println!("Full backup downloaded successfully");
            
            // Now we need to find and download all incremental backups
            // List all backups that have this full backup as ancestor
            let incremental_backups = storage.list_backups_with_ancestor(&backup_id)
                .await
                .map_err(|e| anyhow!("Failed to list incremental backups: {}", e))?;
            
            // Download each incremental backup
            for backup_id in incremental_backups {
                println!("Downloading incremental backup {}...", backup_id);
                
                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path)
                        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;
                }
                
                storage.download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e| anyhow!("Failed to download incremental backup: {}", e))?;
            }
            
            println!("All incremental backups downloaded successfully");
        }
    }
    
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
        ssh_host,
        ssh_user,
        ssh_port,
        ssh_password,
        ssh_key_path,
        ssh_local_port,
        ssh_remote_port,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    println!("Snapshot backup contents for {}:", backup_id);
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let contents = manager
        .list_snapshot_contents(&backup_id)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    for item in contents.split('\n').filter(|s| !s.is_empty()) {
        println!("{}", item);
    }
    Ok(())
}
