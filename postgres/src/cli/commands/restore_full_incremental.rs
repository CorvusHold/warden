use super::{create_storage_provider, restart_postgresql, SshOptions, StorageOptions};
use crate::common::PostgresConfig;
use crate::manager::PostgresManager;
use crate::tunnel_keeper::TunnelKeeper;
use crate::PostgresError;
use anyhow::{anyhow, Result};
use log::{error, info};
use std::path::PathBuf;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn restore_full(
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
        info!("Downloading full backup from remote storage...");
        let storage_instance = create_storage_provider(&storage).await?;
        if let Some(storage) = storage_instance {
            let full_backup_path = backup_dir.join(&full_backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path).map_err(|e: std::io::Error| {
                    anyhow!("Failed to create backup directory: {}", e)
                })?;
            }
            storage
                .download_backup(&full_backup_id, &full_backup_path)
                .await
                .map_err(|e: storage::StorageError| {
                    anyhow!("Failed to download full backup: {}", e)
                })?;
            info!("Full backup downloaded successfully");
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
    info!(
        "Restoring from full backup {} to {:?}...",
        full_backup_id, target_dir
    );
    let full_backup_id = Uuid::parse_str(&full_backup_id).map_err(|e: uuid::Error| anyhow!(e))?;
    let restore = manager
        .restore_full_backup(&full_backup_id, target_dir)
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
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    // If restoring from remote storage, download the backup first
    if storage.remote_storage {
        info!("Downloading full and incremental backups from remote storage...");
        let storage_instance = create_storage_provider(&storage).await?;
        if let Some(storage) = storage_instance {
            // Download full backup
            let full_backup_path = backup_dir.join(&full_backup_id);
            if !full_backup_path.exists() {
                std::fs::create_dir_all(&full_backup_path).map_err(|e: std::io::Error| {
                    anyhow!("Failed to create backup directory: {}", e)
                })?;
            }
            storage
                .download_backup(&full_backup_id, &full_backup_path)
                .await
                .map_err(|e: storage::StorageError| {
                    anyhow!("Failed to download full backup: {}", e)
                })?;
            info!("Full backup downloaded successfully");
            // Download incremental backups
            let incremental_backups = storage
                .list_backups_with_ancestor(&full_backup_id)
                .await
                .map_err(|e: storage::StorageError| {
                    anyhow!("Failed to list incremental backups: {}", e)
                })?;
            for backup_id in incremental_backups {
                info!("Downloading incremental backup {}...", backup_id);
                let backup_path = backup_dir.join(&backup_id);
                if !backup_path.exists() {
                    std::fs::create_dir_all(&backup_path).map_err(|e: std::io::Error| {
                        anyhow!("Failed to create backup directory: {}", e)
                    })?;
                }
                storage
                    .download_backup(&backup_id, &backup_path)
                    .await
                    .map_err(|e: storage::StorageError| {
                        anyhow!("Failed to download incremental backup: {}", e)
                    })?;
            }
            info!("All incremental backups downloaded successfully");
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
    info!(
        "Restoring with incremental backups from {} to {:?}...",
        full_backup_id, target_dir
    );
    let full_backup_id = Uuid::parse_str(&full_backup_id).map_err(|e: uuid::Error| anyhow!(e))?;
    let restore = manager
        .restore_incremental_backup(&full_backup_id, target_dir)
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
