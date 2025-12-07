use anyhow::{anyhow, Result};
use chrono::Utc;
use log::{error, info, warn};
use std::collections::HashMap;
use std::{path::PathBuf, sync::atomic::Ordering};
use uuid::Uuid;

// Import storage module
use storage::{
    BackupMetadata, BackupStatus as StorageBackupStatus, BackupType as StorageBackupType,
    Metadata, PostgresBackupStorage, StorageProviderType,
};

use crate::common::PostgresConfig;
use crate::manager::PostgresManager;
use crate::tunnel_keeper::TunnelKeeper;
use crate::PostgresError;

pub mod backups;
pub mod cluster;
mod full_restore;
pub mod ha;
pub mod migration;
mod pitr;
mod restore_full_incremental;
pub mod retention;
pub mod schedule;
pub mod status;

pub use cluster::{
    cluster_nodes, cluster_protection_groups, cluster_show, cluster_validate,
    format_cluster_overview, format_node_list, format_protection_group_list,
    format_validation_result, ClusterInfo, ClusterOverview, NodeInfo, NodeList, OutputFormat,
    ProtectionGroupInfo, ProtectionGroupList, ValidationResult,
};
pub use full_restore::full_restore as execute_full_restore;
pub use full_restore::FullRestoreOptions;
pub use pitr::{pitr_list, pitr_plan, pitr_restore, PitrPlanResult, PitrStorageOptions};
pub use restore_full_incremental::{restore_full, restore_incremental};
pub use retention::{
    format_retention_plan, retention_apply, retention_init, retention_plan, RetentionOptions,
    RetentionPlanResult,
};
pub use schedule::{schedule_list, schedule_next_runs, schedule_run, schedule_validate};
pub use ha::{execute_ha_clone_node, execute_ha_failover, execute_ha_switchover};
pub use status::{
    execute_backup_status, execute_metrics, execute_pitr_status, execute_status,
    StatusStorageOptions,
};
pub use migration::{
    discover, format_discovery_result, format_generated_config, format_import_result,
    generate_config, import_backup, DatabaseInfo, DiscoveryResult, GenerateConfigOptions,
    GeneratedConfig, ImportBackupOptions, ImportBackupType, ImportResult, ReplicationInfo,
    SshOptions as MigrationSshOptions, StorageOptions as MigrationStorageOptions,
};

/// Result of a successful snapshot backup operation
#[derive(Debug, Clone)]
pub struct SnapshotBackupResult {
    /// Unique backup identifier (UUID)
    pub backup_id: String,
    /// Local path where the backup is stored
    pub local_path: PathBuf,
    /// Remote S3 path/key if uploaded to remote storage
    pub remote_path: Option<String>,
    /// Database name that was backed up
    pub database: String,
    /// Backup start time
    pub start_time: chrono::DateTime<Utc>,
    /// Backup end time
    pub end_time: chrono::DateTime<Utc>,
    /// Size of the backup in bytes
    pub size_bytes: u64,
}

/// Generate a deterministic S3 object key for a backup
/// Format: {prefix}/{database}/{date}/{backup_id}/
pub fn generate_backup_s3_key(
    prefix: Option<&str>,
    database: &str,
    backup_id: &str,
    timestamp: &chrono::DateTime<Utc>,
) -> String {
    let date_str = timestamp.format("%Y-%m-%d").to_string();
    match prefix {
        Some(p) if !p.is_empty() => {
            let p = p.trim_end_matches('/');
            format!("{}/{}/{}/{}", p, database, date_str, backup_id)
        }
        _ => format!("{}/{}/{}", database, date_str, backup_id),
    }
}

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
    labels: HashMap<String, String>,
) -> Result<SnapshotBackupResult> {
    let start_time = Utc::now();
    info!("[snapshot-backup] Starting backup operation");
    info!(
        "[snapshot-backup] Database: {}, Host: {}, Port: {}, Backup dir: {:?}",
        database, host, port, backup_dir
    );

    // Build PostgresConfig, adjusting for SSH tunnel if needed
    let effective_host = if ssh.host.is_some() {
        "localhost".to_string()
    } else {
        host.clone()
    };
    let effective_port = if ssh.host.is_some() {
        ssh.local_port.unwrap_or(6969)
    } else {
        port
    };

    let config = PostgresConfig {
        host: effective_host,
        port: effective_port,
        database: database.clone(),
        user: user.clone(),
        password,
        ssl_mode,
        maintenance_db: None,
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
        info!("[snapshot-backup] Setting up SSH tunnel...");
        let keeper_instance = TunnelKeeper::instance().await;
        let mut keeper = keeper_instance.lock().await;
        if let Err(e) = keeper.setup(&config).await {
            error!("[snapshot-backup] Failed to setup SSH tunnel: {e}");
            return Err(anyhow!("SSH tunnel setup failed: {}", e));
        }
        info!("[snapshot-backup] SSH tunnel established successfully");
    }

    // Ensure backup directory exists
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| anyhow!("Failed to create backup directory: {}", e))?;

    // Perform the backup
    info!("[snapshot-backup] Creating backup...");
    let mut manager = PostgresManager::new(config.clone(), backup_dir.clone())?;
    let backup = manager
        .snapshot_backup()
        .await
        .map_err(|e| anyhow!("Backup failed: {}", e))?;

    let backup_id = backup.id.to_string();
    info!("[snapshot-backup] Backup created: {}", backup_id);

    // Find the actual backup directory
    let actual_backup_path = find_backup_directory(&backup_dir, "snapshot_backup_")?;
    info!(
        "[snapshot-backup] Backup directory: {}",
        actual_backup_path.display()
    );

    // Calculate backup size
    let size_bytes = calculate_directory_size(&actual_backup_path)?;
    info!(
        "[snapshot-backup] Backup size: {} bytes ({:.2} MB)",
        size_bytes,
        size_bytes as f64 / 1024.0 / 1024.0
    );

    // Prepare result
    let mut result = SnapshotBackupResult {
        backup_id: backup_id.clone(),
        local_path: actual_backup_path.clone(),
        remote_path: None,
        database: database.clone(),
        start_time,
        end_time: Utc::now(),
        size_bytes,
    };

    // Write local metadata file
    let local_metadata = create_local_metadata(
        &backup_id,
        &database,
        &host,
        port,
        start_time,
        result.end_time,
        size_bytes,
        backup.server_version.clone(),
        &labels,
    );
    let metadata_path = actual_backup_path.join("backup_metadata.json");
    let metadata_json = serde_json::to_string_pretty(&local_metadata)
        .map_err(|e| anyhow!("Failed to serialize metadata: {}", e))?;
    std::fs::write(&metadata_path, &metadata_json)
        .map_err(|e| anyhow!("Failed to write metadata file: {}", e))?;
    info!(
        "[snapshot-backup] Metadata written to: {}",
        metadata_path.display()
    );

    // Upload to remote storage if configured
    if storage.remote_storage {
        info!("[snapshot-backup] Uploading to remote storage...");
        match upload_to_remote_storage(
            &storage,
            &backup_id,
            &database,
            &actual_backup_path,
            start_time,
            &labels,
            &backup.server_version,
        )
        .await
        {
            Ok(remote_key) => {
                result.remote_path = Some(remote_key.clone());
                info!(
                    "[snapshot-backup] Successfully uploaded to remote storage: {}",
                    remote_key
                );
            }
            Err(e) => {
                // Log error but don't fail the backup - local backup is still valid
                error!(
                    "[snapshot-backup] Failed to upload to remote storage: {}. Local backup is still available.",
                    e
                );
                warn!("[snapshot-backup] Backup completed locally but remote upload failed");
            }
        }
    }

    // Close SSH tunnel after all operations
    if config.ssh_host.is_some() {
        info!("[snapshot-backup] Closing SSH tunnel...");
        let keeper_instance = TunnelKeeper::instance().await;
        let is_active = {
            let keeper = keeper_instance.lock().await;
            keeper.is_active.load(Ordering::SeqCst)
        };
        if is_active {
            let mut keeper = keeper_instance.lock().await;
            if let Err(e) = keeper.close().await {
                warn!("[snapshot-backup] Error closing SSH tunnel: {e}");
            } else {
                info!("[snapshot-backup] SSH tunnel closed");
            }
        }
    }

    info!(
        "[snapshot-backup] Backup completed successfully. ID: {}, Duration: {}s",
        result.backup_id,
        (result.end_time - result.start_time).num_seconds()
    );

    Ok(result)
}

/// Find the most recent backup directory matching a prefix
fn find_backup_directory(backup_dir: &PathBuf, prefix: &str) -> Result<PathBuf> {
    let entries = std::fs::read_dir(backup_dir)
        .map_err(|e| anyhow!("Failed to read backup directory: {}", e))?;

    let mut matching_dirs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir()
                && e.file_name()
                    .to_string_lossy()
                    .starts_with(prefix)
        })
        .collect();

    // Sort by name (which includes timestamp) to get the most recent
    matching_dirs.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    matching_dirs
        .first()
        .map(|e| e.path())
        .ok_or_else(|| anyhow!("No backup directory found matching prefix '{}'", prefix))
}

/// Calculate total size of a directory
fn calculate_directory_size(path: &PathBuf) -> Result<u64> {
    let mut total_size = 0u64;
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            total_size += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(total_size)
}

/// Create local metadata structure
#[allow(clippy::too_many_arguments)]
fn create_local_metadata(
    backup_id: &str,
    database: &str,
    host: &str,
    port: u16,
    start_time: chrono::DateTime<Utc>,
    end_time: chrono::DateTime<Utc>,
    size_bytes: u64,
    server_version: String,
    labels: &HashMap<String, String>,
) -> serde_json::Value {
    serde_json::json!({
        "backup_id": backup_id,
        "backup_type": "snapshot",
        "database": database,
        "host": host,
        "port": port,
        "start_time": start_time.to_rfc3339(),
        "end_time": end_time.to_rfc3339(),
        "duration_seconds": (end_time - start_time).num_seconds(),
        "size_bytes": size_bytes,
        "server_version": server_version,
        "labels": labels,
        "created_by": "warden",
        "version": "1.0"
    })
}

/// Upload backup to remote S3-compatible storage
#[allow(clippy::too_many_arguments)]
async fn upload_to_remote_storage(
    storage_opts: &StorageOptions,
    backup_id: &str,
    database: &str,
    backup_path: &PathBuf,
    timestamp: chrono::DateTime<Utc>,
    labels: &HashMap<String, String>,
    server_version: &str,
) -> Result<String> {
    let storage = create_storage_provider(storage_opts)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    // Generate deterministic S3 key
    let s3_key = generate_backup_s3_key(
        storage_opts.prefix.as_deref(),
        database,
        backup_id,
        &timestamp,
    );

    // Build object metadata
    let mut obj_metadata = Metadata::new();
    obj_metadata.insert("backup_id".to_string(), backup_id.to_string());
    obj_metadata.insert("backup_type".to_string(), "snapshot".to_string());
    obj_metadata.insert("database".to_string(), database.to_string());
    obj_metadata.insert("timestamp".to_string(), timestamp.to_rfc3339());
    for (key, value) in labels {
        obj_metadata.insert(format!("label_{}", key), value.clone());
    }

    // Find and upload the logical backup file
    let dump_file = find_dump_file(backup_path, database)?;
    info!(
        "[snapshot-backup] Uploading logical backup: {}",
        dump_file.display()
    );

    storage
        .upload_logical_backup(backup_id, &dump_file, Some(obj_metadata.clone()))
        .await
        .map_err(|e| anyhow!("Failed to upload logical backup: {}", e))?;

    // Create and upload backup metadata
    let backup_metadata = BackupMetadata {
        id: backup_id.to_string(),
        backup_type: StorageBackupType::Snapshot,
        status: StorageBackupStatus::Completed,
        start_time: timestamp,
        end_time: Some(Utc::now()),
        base_backup_id: None,
        wal_start: None,
        wal_end: None,
        size_bytes: calculate_directory_size(backup_path)?,
        server_version: server_version.to_string(),
        checksum: None,
        files: vec![],
        tags: labels.iter().map(|(k, v)| format!("{}={}", k, v)).collect(),
        pinned: false,
        encrypted: None, // TODO: Set based on encryption config
        encryption_algorithm: None,
    };

    storage
        .upload_backup_metadata(backup_id, &backup_metadata)
        .await
        .map_err(|e| anyhow!("Failed to upload backup metadata: {}", e))?;

    Ok(format!(
        "s3://{}/{}",
        storage_opts.bucket.as_deref().unwrap_or("unknown"),
        s3_key
    ))
}

/// Find the dump file in the backup directory
fn find_dump_file(backup_path: &PathBuf, database: &str) -> Result<PathBuf> {
    // Try database-specific dump file first
    let dump_file = backup_path.join(format!("{}.dump", database));
    if dump_file.exists() {
        return Ok(dump_file);
    }

    // Try generic pg_dump.dump
    let alt_dump_file = backup_path.join("pg_dump.dump");
    if alt_dump_file.exists() {
        return Ok(alt_dump_file);
    }

    // Try any .dump file
    for entry in std::fs::read_dir(backup_path)
        .map_err(|e| anyhow!("Failed to read backup directory: {}", e))?
    {
        let entry = entry.map_err(|e| anyhow!("Failed to read directory entry: {}", e))?;
        let path = entry.path();
        if path.extension().map(|e| e == "dump").unwrap_or(false) {
            return Ok(path);
        }
    }

    Err(anyhow!(
        "No dump file found in backup directory: {}",
        backup_path.display()
    ))
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

/// Options for multi-tenant storage organization.
///
/// These options control the hierarchical key layout in S3/MinIO storage.
/// When tenant/cluster/protection_group are set, backups are organized as:
/// `<tenant>/<cluster>/<protection_group>/<database>/<backup_id>/...`
///
/// When not set, the legacy flat layout is used:
/// `<prefix>/<backup_id>/...`
#[derive(Clone, Debug, Default)]
pub struct MultiTenantOptions {
    /// Tenant identifier (organization/project)
    pub tenant: Option<String>,
    /// Cluster identifier from cluster.yaml
    pub cluster: Option<String>,
    /// Protection group identifier from cluster.yaml
    pub protection_group: Option<String>,
    /// Include legacy backups in discovery operations
    pub include_legacy: bool,
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
    /// Multi-tenant organization options
    pub multi_tenant: MultiTenantOptions,
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
        maintenance_db: None,
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
            let dump_file = actual_backup_path.join(format!("{database}.dump"));
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
                log::error!("Warning: Error closing SSH tunnel: {e}");
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
        maintenance_db: None,
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
            let dump_file = actual_backup_path.join(format!("{database}.dump"));
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
                error!("Warning: Error closing SSH tunnel: {e}");
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
                println!(
                    "   Size: {:.2} GB",
                    backup.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0
                );
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
            println!(
                "Total size: {:.2} GB",
                total_size as f64 / 1024.0 / 1024.0 / 1024.0
            );

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
        maintenance_db: None,
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
                info!("Downloading incremental backup {backup_id}...");

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
        maintenance_db: None,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    info!("Restoring with incremental backups from {full_backup_id} to {target_dir:?}...");
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
                info!("Downloading incremental backup {backup_id}...");
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
        maintenance_db: None,
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
    info!("Restoring to point in time {target_time} from {full_backup_id} to {target_dir:?}...");
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
                error!("Warning: Error closing SSH tunnel: {e}");
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
        maintenance_db: None,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    info!("Restoring from snapshot backup {backup_id} to {target_dir:?}...");
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
            info!("Restarting PostgreSQL in Docker container {id}...");
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
            info!("Restarting PostgreSQL in Kubernetes pod {id}...");
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
                    info!("Auto-restart not supported on {os} operating system. Please restart PostgreSQL manually.");
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
                        "PostgreSQL successfully restarted using pg_ctl with data directory: {data_dir}"
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
        let service_name = format!("postgresql-{version}");
        if let Ok(output) = std::process::Command::new("systemctl")
            .args(["restart", &service_name])
            .output()
        {
            if output.status.success() {
                info!("PostgreSQL {version} successfully restarted using systemctl");
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
                        "PostgreSQL successfully restarted using pg_ctl with data directory: {data_dir}"
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
                info!("Downloading incremental backup {backup_id}...");

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
        maintenance_db: None,
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    info!("Snapshot backup contents for {backup_id}:");
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e: uuid::Error| anyhow::anyhow!(e))?;
    let contents = manager
        .list_snapshot_contents(&backup_id)
        .await
        .map_err(|e: PostgresError| anyhow::anyhow!(e))?;
    for item in contents.split('\n').filter(|s| !s.is_empty()) {
        info!("{item}");
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
    println!(
        "Size: {} bytes ({:.2} GB)",
        metadata.size_bytes,
        metadata.size_bytes as f64 / 1024.0 / 1024.0 / 1024.0
    );
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

    info!(
        "Backup {} downloaded successfully to {:?}",
        backup_id, target_dir
    );

    Ok(())
}

/// Initialize or update retention policy
pub async fn init_retention_policy(storage: StorageOptions, policy_file: PathBuf) -> Result<()> {
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

    info!(
        "Retention policy saved successfully to bucket {}",
        storage.bucket.unwrap_or_default()
    );

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
        .ok_or_else(|| {
            anyhow!("No retention policy found. Use 'init-retention-policy' to create one.")
        })?;

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
        _ => {
            println!("\n=== Purge Evaluation ===");
            println!("Timestamp: {}", evaluation.timestamp);
            println!("Total backups: {}", evaluation.total_backups);
            println!("To keep: {}", evaluation.to_keep.len());
            println!("To delete: {}", evaluation.to_delete.len());
            println!(
                "Space to free: {} bytes ({:.2} GB)",
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
                    println!(
                        "  🗑️  {} ({:?}) - {} - {:.2} GB",
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
                    println!(
                        "  ✅ {} ({:?}) - {}",
                        decision.backup_id, decision.backup_type, decision.reason
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
        .ok_or_else(|| {
            anyhow!("No retention policy found. Use 'init-retention-policy' to create one.")
        })?;

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
    println!(
        "Space to free: {} bytes ({:.2} GB)",
        evaluation.estimated_space_freed,
        evaluation.estimated_space_freed as f64 / 1024.0 / 1024.0 / 1024.0
    );

    if !evaluation.to_delete.is_empty() {
        println!("\nBackups to be deleted:");
        for decision in &evaluation.to_delete {
            println!(
                "  - {} ({:?}) - {}",
                decision.backup_id, decision.backup_type, decision.reason
            );
        }
    }

    // Confirm if applying and confirmation required
    if apply && !yes && policy.safety.require_confirmation {
        use std::io::{self, Write};
        print!(
            "\nAre you sure you want to delete {} backups? (yes/no): ",
            evaluation.to_delete.len()
        );
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
    println!(
        "Space freed: {} bytes ({:.2} GB)",
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

/// Reconstruct metadata for existing backups
pub async fn reconstruct_metadata(
    storage: StorageOptions,
    server_version: String,
    dry_run: bool,
    skip_checksums: bool,
) -> Result<()> {
    use chrono::Utc;
    use storage::{BackupFile, BackupMetadata, BackupStatus, BackupType};

    info!("Scanning for backups without metadata...");

    let storage_instance = create_storage_provider(&storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    // List all objects in the bucket to find backup directories
    let prefix = storage.prefix.clone().unwrap_or_default();
    let all_objects = storage_instance
        .list_all_objects()
        .await
        .map_err(|e| anyhow!("Failed to list objects: {}", e))?;

    info!("Found {} total objects in storage", all_objects.len());

    // Group objects by backup ID (first directory level after prefix)
    use std::collections::HashMap;
    let mut backup_dirs: HashMap<String, Vec<_>> = HashMap::new();

    for obj in &all_objects {
        // Skip metadata files
        if obj.key.ends_with("/backup_metadata.json") || obj.key.ends_with(".retention_policy") {
            continue;
        }

        // Extract backup ID from path
        let path_after_prefix = if prefix.is_empty() {
            obj.key.as_str()
        } else {
            obj.key
                .strip_prefix(&format!("{}/", prefix))
                .unwrap_or(&obj.key)
        };

        if let Some(backup_id) = path_after_prefix.split('/').next() {
            if !backup_id.is_empty() {
                backup_dirs
                    .entry(backup_id.to_string())
                    .or_default()
                    .push(obj.clone());
            }
        }
    }

    info!(
        "Identified {} potential backup directories",
        backup_dirs.len()
    );

    // Check which ones already have metadata
    let mut backups_without_metadata = Vec::new();
    for backup_id in backup_dirs.keys() {
        let metadata_key = if prefix.is_empty() {
            format!("{}/backup_metadata.json", backup_id)
        } else {
            format!("{}/{}/backup_metadata.json", prefix, backup_id)
        };

        let has_metadata = all_objects.iter().any(|obj| obj.key == metadata_key);
        if !has_metadata {
            backups_without_metadata.push(backup_id.clone());
        }
    }

    info!(
        "Found {} backups without metadata",
        backups_without_metadata.len()
    );

    if backups_without_metadata.is_empty() {
        println!("✅ All backups already have metadata!");
        return Ok(());
    }

    // Reconstruct metadata for each backup
    let mut reconstructed_count = 0;
    for backup_id in &backups_without_metadata {
        let objects = &backup_dirs[backup_id];

        println!("\n📦 Processing backup: {}", backup_id);
        println!("   Found {} files", objects.len());

        // Calculate total size
        let total_size: u64 = objects.iter().filter_map(|obj| obj.size).sum();
        println!(
            "   Total size: {} bytes ({:.2} GB)",
            total_size,
            total_size as f64 / 1024.0 / 1024.0 / 1024.0
        );

        // Get timestamps from objects
        let timestamps: Vec<_> = objects.iter().filter_map(|obj| obj.last_modified).collect();
        let start_time = timestamps.iter().min().copied().unwrap_or(Utc::now());
        let end_time = timestamps.iter().max().copied();
        println!("   Start time: {}", start_time);
        if let Some(end) = end_time {
            println!("   End time: {}", end);
        }

        // Infer backup type from directory structure
        let has_base_backup = objects.iter().any(|obj| obj.key.contains("/base"));
        let has_pg_wal = objects.iter().any(|obj| obj.key.contains("/pg_wal"));
        let backup_type = if has_base_backup && has_pg_wal {
            BackupType::Full
        } else if has_pg_wal {
            BackupType::Incremental
        } else {
            BackupType::Snapshot
        };
        println!("   Inferred type: {:?}", backup_type);

        // Build files list
        let files: Vec<BackupFile> = objects
            .iter()
            .map(|obj| {
                let name = if prefix.is_empty() {
                    obj.key
                        .strip_prefix(&format!("{}/", backup_id))
                        .unwrap_or(&obj.key)
                        .to_string()
                } else {
                    obj.key
                        .strip_prefix(&format!("{}/{}/", prefix, backup_id))
                        .unwrap_or(&obj.key)
                        .to_string()
                };
                BackupFile {
                    name,
                    size: obj.size.unwrap_or(0),
                    checksum: if skip_checksums {
                        None
                    } else {
                        obj.etag.clone()
                    },
                }
            })
            .collect();

        // Create metadata
        let metadata = BackupMetadata {
            id: backup_id.clone(),
            backup_type,
            status: BackupStatus::Completed,
            start_time,
            end_time,
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: total_size,
            server_version: server_version.clone(),
            checksum: None,
            files,
            tags: vec!["reconstructed".to_string()],
            pinned: false,
            encrypted: None, // Unknown for reconstructed metadata
            encryption_algorithm: None,
        };

        if dry_run {
            println!("   [DRY RUN] Would create metadata file");
        } else {
            // Save metadata
            storage_instance
                .upload_backup_metadata(backup_id, &metadata)
                .await
                .map_err(|e| anyhow!("Failed to save metadata for {}: {}", backup_id, e))?;
            println!("   ✅ Created metadata file");
            reconstructed_count += 1;
        }
    }

    println!("\n{}", "=".repeat(60));
    if dry_run {
        println!(
            "🔍 Dry run complete: Found {} backups that need metadata",
            backups_without_metadata.len()
        );
        println!("   Run without --dry-run to create metadata files");
    } else {
        println!(
            "✅ Successfully reconstructed metadata for {} backups",
            reconstructed_count
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_generate_backup_s3_key_with_prefix() {
        let timestamp = Utc.with_ymd_and_hms(2025, 12, 6, 14, 30, 0).unwrap();
        let key = generate_backup_s3_key(
            Some("postgres/prod"),
            "mydb",
            "abc123-uuid",
            &timestamp,
        );
        assert_eq!(key, "postgres/prod/mydb/2025-12-06/abc123-uuid");
    }

    #[test]
    fn test_generate_backup_s3_key_without_prefix() {
        let timestamp = Utc.with_ymd_and_hms(2025, 1, 15, 8, 0, 0).unwrap();
        let key = generate_backup_s3_key(None, "testdb", "backup-id-456", &timestamp);
        assert_eq!(key, "testdb/2025-01-15/backup-id-456");
    }

    #[test]
    fn test_generate_backup_s3_key_with_trailing_slash_prefix() {
        let timestamp = Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap();
        let key = generate_backup_s3_key(
            Some("backups/"),
            "database",
            "id-789",
            &timestamp,
        );
        assert_eq!(key, "backups/database/2025-06-01/id-789");
    }

    #[test]
    fn test_generate_backup_s3_key_empty_prefix() {
        let timestamp = Utc.with_ymd_and_hms(2025, 3, 20, 0, 0, 0).unwrap();
        let key = generate_backup_s3_key(Some(""), "db", "backup", &timestamp);
        assert_eq!(key, "db/2025-03-20/backup");
    }

    #[test]
    fn test_create_local_metadata_structure() {
        let mut labels = HashMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        labels.insert("cluster".to_string(), "primary".to_string());

        let start = Utc.with_ymd_and_hms(2025, 12, 6, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 12, 6, 10, 5, 30).unwrap();

        let metadata = create_local_metadata(
            "test-backup-id",
            "mydb",
            "db.example.com",
            5432,
            start,
            end,
            1024 * 1024 * 100, // 100 MB
            "15.2".to_string(),
            &labels,
        );

        assert_eq!(metadata["backup_id"], "test-backup-id");
        assert_eq!(metadata["backup_type"], "snapshot");
        assert_eq!(metadata["database"], "mydb");
        assert_eq!(metadata["host"], "db.example.com");
        assert_eq!(metadata["port"], 5432);
        assert_eq!(metadata["size_bytes"], 104857600);
        assert_eq!(metadata["server_version"], "15.2");
        assert_eq!(metadata["duration_seconds"], 330);
        assert_eq!(metadata["labels"]["env"], "prod");
        assert_eq!(metadata["labels"]["cluster"], "primary");
        assert_eq!(metadata["created_by"], "warden");
        assert_eq!(metadata["version"], "1.0");
    }

    #[test]
    fn test_find_backup_directory() {
        let temp_dir = TempDir::new().unwrap();
        let backup_dir = temp_dir.path().to_path_buf();

        // Create some test directories
        std::fs::create_dir(backup_dir.join("snapshot_backup_2025-12-06T10-00-00")).unwrap();
        std::fs::create_dir(backup_dir.join("snapshot_backup_2025-12-06T11-00-00")).unwrap();
        std::fs::create_dir(backup_dir.join("full_backup_2025-12-05")).unwrap();
        std::fs::create_dir(backup_dir.join("other_dir")).unwrap();

        // Should find the most recent snapshot backup
        let result = find_backup_directory(&backup_dir, "snapshot_backup_").unwrap();
        assert!(result.file_name().unwrap().to_string_lossy().contains("2025-12-06T11-00-00"));

        // Should find full backup
        let result = find_backup_directory(&backup_dir, "full_backup_").unwrap();
        assert!(result.file_name().unwrap().to_string_lossy().contains("full_backup_"));

        // Should fail for non-existent prefix
        let result = find_backup_directory(&backup_dir, "nonexistent_");
        assert!(result.is_err());
    }

    #[test]
    fn test_calculate_directory_size() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().to_path_buf();

        // Create test files
        std::fs::write(dir_path.join("file1.txt"), "Hello, World!").unwrap(); // 13 bytes
        std::fs::write(dir_path.join("file2.txt"), "Test data").unwrap(); // 9 bytes
        std::fs::create_dir(dir_path.join("subdir")).unwrap();
        std::fs::write(dir_path.join("subdir/file3.txt"), "Nested file content").unwrap(); // 19 bytes

        let size = calculate_directory_size(&dir_path).unwrap();
        assert_eq!(size, 13 + 9 + 19);
    }

    #[test]
    fn test_find_dump_file_database_specific() {
        let temp_dir = TempDir::new().unwrap();
        let backup_path = temp_dir.path().to_path_buf();

        // Create database-specific dump file
        std::fs::write(backup_path.join("mydb.dump"), "dump content").unwrap();

        let result = find_dump_file(&backup_path, "mydb").unwrap();
        assert_eq!(result.file_name().unwrap(), "mydb.dump");
    }

    #[test]
    fn test_find_dump_file_generic() {
        let temp_dir = TempDir::new().unwrap();
        let backup_path = temp_dir.path().to_path_buf();

        // Create generic dump file (no database-specific one)
        std::fs::write(backup_path.join("pg_dump.dump"), "dump content").unwrap();

        let result = find_dump_file(&backup_path, "mydb").unwrap();
        assert_eq!(result.file_name().unwrap(), "pg_dump.dump");
    }

    #[test]
    fn test_find_dump_file_any_dump() {
        let temp_dir = TempDir::new().unwrap();
        let backup_path = temp_dir.path().to_path_buf();

        // Create a dump file with different name
        std::fs::write(backup_path.join("backup.dump"), "dump content").unwrap();

        let result = find_dump_file(&backup_path, "mydb").unwrap();
        assert_eq!(result.extension().unwrap(), "dump");
    }

    #[test]
    fn test_find_dump_file_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let backup_path = temp_dir.path().to_path_buf();

        // Create non-dump files
        std::fs::write(backup_path.join("data.txt"), "not a dump").unwrap();

        let result = find_dump_file(&backup_path, "mydb");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No dump file found"));
    }

    #[test]
    fn test_ssh_options_default() {
        let ssh = SshOptions::default();
        assert!(ssh.host.is_none());
        assert!(ssh.user.is_none());
        assert!(ssh.port.is_none());
        assert!(ssh.password.is_none());
        assert!(ssh.key_path.is_none());
        assert!(ssh.local_port.is_none());
        assert!(ssh.remote_port.is_none());
    }

    #[test]
    fn test_storage_options_default() {
        let storage = StorageOptions::default();
        assert!(!storage.remote_storage);
        assert!(storage.provider_type.is_none());
        assert!(storage.bucket.is_none());
        assert!(storage.prefix.is_none());
        assert!(storage.region.is_none());
        assert!(storage.endpoint.is_none());
        assert!(storage.access_key.is_none());
        assert!(storage.secret_key.is_none());
    }

    #[test]
    fn test_snapshot_backup_result_structure() {
        let result = SnapshotBackupResult {
            backup_id: "test-id".to_string(),
            local_path: PathBuf::from("/backups/test"),
            remote_path: Some("s3://bucket/key".to_string()),
            database: "testdb".to_string(),
            start_time: Utc::now(),
            end_time: Utc::now(),
            size_bytes: 1024,
        };

        assert_eq!(result.backup_id, "test-id");
        assert_eq!(result.database, "testdb");
        assert!(result.remote_path.is_some());
    }
}
