//! PostgreSQL DataSource plugin implementation.
//!
//! This module implements the `DataSource` trait for PostgreSQL,
//! providing a unified interface for backup, restore, and management
//! operations that integrates with Warden's plugin architecture.

use async_trait::async_trait;
use log::{error, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use common::datasource::{
    BackupConfig, BackupFilter, BackupMetadata, BackupResult, BackupStatus, BackupType,
    DataSource, DataSourceCapabilities, DataSourceError, DataSourceStatus, DiscoverConfig,
    DiscoverResult, PitrConfig, RestoreConfig, RestoreResult, RestoreStatus, StatusConfig,
};

use crate::common::PostgresConfig;
use crate::manager::PostgresManager;
use crate::PostgresError;

/// PostgreSQL data source plugin.
///
/// This is the reference implementation of the `DataSource` trait for PostgreSQL.
/// It provides comprehensive backup, restore, and PITR capabilities.
///
/// # Features
///
/// - Full backups (pg_basebackup)
/// - Incremental backups (WAL archiving)
/// - Snapshot backups (pg_dump)
/// - Point-in-Time Recovery
/// - SSH tunnel support
/// - S3-compatible remote storage
/// - High Availability orchestration
///
/// # Example
///
/// ```rust,ignore
/// use postgres::datasource::PostgresDataSource;
/// use common::datasource::{DataSource, DiscoverConfig, ConnectionParams};
///
/// let datasource = PostgresDataSource::new();
///
/// // Discover a PostgreSQL server
/// let config = DiscoverConfig::new(
///     ConnectionParams::new("localhost", 5432)
///         .with_database("mydb")
///         .with_user("postgres")
/// );
/// let result = datasource.discover(&config).await?;
/// println!("Server version: {:?}", result.server_version);
/// ```
pub struct PostgresDataSource {
    /// Internal state (if needed for caching, connection pooling, etc.)
    _state: Arc<Mutex<PostgresDataSourceState>>,
}

/// Internal state for the PostgreSQL data source
#[allow(dead_code)] // Reserved for future caching/connection pooling
struct PostgresDataSourceState {
    /// Cached server version (if discovered)
    server_version: Option<String>,
}

impl PostgresDataSource {
    /// Create a new PostgreSQL data source plugin.
    pub fn new() -> Self {
        Self {
            _state: Arc::new(Mutex::new(PostgresDataSourceState {
                server_version: None,
            })),
        }
    }

    /// Convert common ConnectionParams to PostgresConfig
    fn to_postgres_config(
        connection: &common::datasource::ConnectionParams,
        ssh_tunnel: Option<&common::datasource::SshTunnelConfig>,
    ) -> PostgresConfig {
        PostgresConfig {
            host: connection.host.clone(),
            port: connection.port,
            database: connection.database.clone().unwrap_or_else(|| "postgres".to_string()),
            user: connection.user.clone().unwrap_or_else(|| "postgres".to_string()),
            password: connection.password.clone(),
            ssl_mode: connection.ssl_mode.clone(),
            maintenance_db: connection.options.get("maintenance_db").cloned(),
            ssh_host: ssh_tunnel.map(|t| t.ssh_host.clone()),
            ssh_user: ssh_tunnel.map(|t| t.ssh_user.clone()),
            ssh_port: ssh_tunnel.map(|t| t.ssh_port),
            ssh_password: ssh_tunnel.and_then(|t| t.ssh_password.clone()),
            ssh_key_path: ssh_tunnel.and_then(|t| t.ssh_key_path.as_ref().map(|p| p.to_string_lossy().to_string())),
            ssh_local_port: ssh_tunnel.and_then(|t| t.local_port),
            ssh_remote_port: ssh_tunnel.map(|t| t.remote_port),
        }
    }

    /// Convert common BackupType to postgres BackupType
    #[allow(dead_code)] // Will be used when backup command is fully integrated
    fn to_postgres_backup_type(backup_type: BackupType) -> crate::common::BackupType {
        match backup_type {
            BackupType::Full => crate::common::BackupType::Full,
            BackupType::Incremental => crate::common::BackupType::Incremental,
            BackupType::Snapshot => crate::common::BackupType::Snapshot,
            BackupType::Differential => crate::common::BackupType::Incremental, // Map to incremental
        }
    }

    /// Convert postgres Backup to common BackupMetadata
    fn to_backup_metadata(backup: &crate::common::Backup) -> BackupMetadata {
        let backup_type = match backup.backup_type {
            crate::common::BackupType::Full => BackupType::Full,
            crate::common::BackupType::Incremental => BackupType::Incremental,
            crate::common::BackupType::Snapshot => BackupType::Snapshot,
        };

        let status = match backup.status {
            crate::common::BackupStatus::InProgress => BackupStatus::InProgress,
            crate::common::BackupStatus::Completed => BackupStatus::Completed,
            crate::common::BackupStatus::Failed => BackupStatus::Failed,
        };

        let mut extra = HashMap::new();
        if let Some(wal_start) = &backup.wal_start {
            extra.insert("wal_start".to_string(), serde_json::Value::String(wal_start.clone()));
        }
        if let Some(wal_end) = &backup.wal_end {
            extra.insert("wal_end".to_string(), serde_json::Value::String(wal_end.clone()));
        }

        BackupMetadata {
            backup_id: backup.id,
            backup_type,
            status,
            start_time: backup.start_time,
            end_time: backup.end_time,
            size_bytes: backup.size_bytes,
            local_path: Some(backup.backup_path.clone()),
            remote_path: None, // Set by caller if uploaded
            server_version: Some(backup.server_version.clone()),
            datasource_type: "postgresql".to_string(),
            database: None, // Could be extracted from path
            labels: HashMap::new(),
            error_message: backup.error_message.clone(),
            base_backup_id: backup.base_backup_id,
            extra,
        }
    }

    /// Convert PostgresError to DataSourceError
    fn convert_error(err: PostgresError) -> DataSourceError {
        match err {
            PostgresError::ConnectionError(msg) => DataSourceError::Connection(msg),
            PostgresError::BackupError(msg) => DataSourceError::Backup(msg),
            PostgresError::BackupNotFound(id) => DataSourceError::BackupNotFound(id),
            PostgresError::RestoreError(msg) => DataSourceError::Restore(msg),
            PostgresError::WalError(msg) => DataSourceError::Pitr(msg),
            PostgresError::PermissionError(msg) => DataSourceError::Authentication(msg),
            PostgresError::Io(err) => DataSourceError::Io(err),
            PostgresError::Postgres(err) => DataSourceError::Connection(err.to_string()),
            PostgresError::MissingPassword => DataSourceError::Authentication("Missing password".to_string()),
            PostgresError::Anyhow(err) => DataSourceError::Internal(err.to_string()),
            PostgresError::Ssh(err) => DataSourceError::SshTunnel(err.to_string()),
        }
    }
}

impl Default for PostgresDataSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataSource for PostgresDataSource {
    fn name(&self) -> &str {
        "postgresql"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &str {
        "PostgreSQL backup and restore with PITR support"
    }

    async fn discover(&self, config: &DiscoverConfig) -> Result<DiscoverResult, DataSourceError> {
        info!("Discovering PostgreSQL server at {}:{}", config.connection.host, config.connection.port);
        let start = Instant::now();

        let pg_config = Self::to_postgres_config(&config.connection, config.ssh_tunnel.as_ref());

        // Try to connect and get server information
        let conn_string = pg_config.connection_string();
        
        let (client, connection) = tokio_postgres::connect(&conn_string, tokio_postgres::NoTls)
            .await
            .map_err(|e| DataSourceError::Connection(e.to_string()))?;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("PostgreSQL connection error: {}", e);
            }
        });

        // Get server version
        let version_row = client
            .query_one("SELECT version()", &[])
            .await
            .map_err(|e| DataSourceError::Connection(e.to_string()))?;
        let version: String = version_row.get(0);

        // Get list of databases
        let db_rows = client
            .query(
                "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
                &[],
            )
            .await
            .map_err(|e| DataSourceError::Connection(e.to_string()))?;

        let databases: Vec<String> = db_rows.iter().map(|row| row.get(0)).collect();

        // Get additional metadata
        let mut metadata = HashMap::new();
        
        // Get PostgreSQL version number
        let pg_version_row = client
            .query_one("SHOW server_version", &[])
            .await
            .map_err(|e| DataSourceError::Connection(e.to_string()))?;
        let pg_version: String = pg_version_row.get(0);
        metadata.insert("server_version".to_string(), pg_version);

        // Check if this is a primary or replica
        let is_recovery_row = client
            .query_one("SELECT pg_is_in_recovery()", &[])
            .await
            .map_err(|e| DataSourceError::Connection(e.to_string()))?;
        let is_recovery: bool = is_recovery_row.get(0);
        metadata.insert("is_replica".to_string(), is_recovery.to_string());

        let latency = start.elapsed().as_millis() as u64;

        info!("PostgreSQL discovery completed in {}ms", latency);

        Ok(DiscoverResult {
            connected: true,
            server_version: Some(version),
            metadata,
            databases,
            latency_ms: Some(latency),
        })
    }

    async fn backup(&self, config: &BackupConfig) -> Result<BackupResult, DataSourceError> {
        let start = Instant::now();
        info!(
            "Starting {:?} backup for PostgreSQL at {}:{}",
            config.backup_type, config.connection.host, config.connection.port
        );

        let pg_config = Self::to_postgres_config(&config.connection, config.ssh_tunnel.as_ref());
        
        let mut manager = PostgresManager::new(pg_config, config.backup_dir.clone())
            .map_err(Self::convert_error)?;

        let backup = match config.backup_type {
            BackupType::Full => manager.full_backup().await.map_err(Self::convert_error)?,
            BackupType::Incremental => manager.incremental_backup().await.map_err(Self::convert_error)?,
            BackupType::Snapshot => manager.snapshot_backup().await.map_err(Self::convert_error)?,
            BackupType::Differential => {
                // Map differential to incremental for PostgreSQL
                manager.incremental_backup().await.map_err(Self::convert_error)?
            }
        };

        let duration = start.elapsed();
        let metadata = Self::to_backup_metadata(&backup);

        info!(
            "Backup {} completed in {:?}",
            backup.id, duration
        );

        Ok(BackupResult {
            backup_id: backup.id,
            backup_type: config.backup_type,
            local_path: backup.backup_path.clone(),
            remote_path: None, // Set by caller after upload
            metadata,
            duration,
        })
    }

    async fn list_backups(
        &self,
        filter: &BackupFilter,
    ) -> Result<Vec<BackupMetadata>, DataSourceError> {
        info!("Listing PostgreSQL backups with filter: {:?}", filter);

        // For now, we need a backup_dir to list backups
        // In a full implementation, this would also query remote storage
        
        // This is a simplified implementation - in practice, you'd want to:
        // 1. Check local catalog
        // 2. Query remote storage if configured
        // 3. Merge and deduplicate results
        
        // Return empty list if no backup directory is specified
        // The actual listing is done through the CLI commands which have access to the backup_dir
        Ok(vec![])
    }

    async fn restore(&self, config: &RestoreConfig) -> Result<RestoreResult, DataSourceError> {
        let start = Instant::now();
        info!(
            "Starting restore of backup {} to {:?}",
            config.backup_id, config.target_dir
        );

        // We need a backup directory to find the backup
        let backup_dir = config.backup_dir.clone().ok_or_else(|| {
            DataSourceError::Configuration("backup_dir is required for restore".to_string())
        })?;

        let pg_config = config.connection.as_ref().map(|c| {
            Self::to_postgres_config(c, config.ssh_tunnel.as_ref())
        }).unwrap_or_else(|| {
            PostgresConfig {
                host: "localhost".to_string(),
                port: 5432,
                database: "postgres".to_string(),
                user: "postgres".to_string(),
                password: None,
                ssl_mode: None,
                maintenance_db: None,
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_password: None,
                ssh_key_path: None,
                ssh_local_port: None,
                ssh_remote_port: None,
            }
        });

        let mut manager = PostgresManager::new(pg_config, backup_dir)
            .map_err(Self::convert_error)?;

        // Get backup info to determine type
        let backup = manager
            .get_backup(&config.backup_id)
            .ok_or(DataSourceError::BackupNotFound(config.backup_id))?
            .clone();

        let restore = match backup.backup_type {
            crate::common::BackupType::Full => {
                manager
                    .restore_full_backup(&config.backup_id, config.target_dir.clone())
                    .await
                    .map_err(Self::convert_error)?
            }
            crate::common::BackupType::Snapshot => {
                manager
                    .restore_snapshot_backup(&config.backup_id, config.target_dir.clone())
                    .await
                    .map_err(Self::convert_error)?
            }
            crate::common::BackupType::Incremental => {
                // For incremental, we need the base backup ID
                let base_id = backup.base_backup_id.ok_or_else(|| {
                    DataSourceError::Restore("Incremental backup missing base_backup_id".to_string())
                })?;
                manager
                    .restore_incremental_backup(&base_id, config.target_dir.clone())
                    .await
                    .map_err(Self::convert_error)?
            }
        };

        let duration = start.elapsed();

        let status = match restore.status {
            crate::common::RestoreStatus::Completed => RestoreStatus::Completed,
            crate::common::RestoreStatus::Failed => RestoreStatus::Failed,
            crate::common::RestoreStatus::InProgress => RestoreStatus::InProgress,
        };

        info!(
            "Restore {} completed in {:?}",
            restore.id, duration
        );

        Ok(RestoreResult {
            restore_id: restore.id,
            backup_id: config.backup_id,
            status,
            restore_path: config.target_dir.clone(),
            duration,
            service_restarted: config.auto_restart,
            target_time: None,
            error_message: restore.error_message,
        })
    }

    fn supports_pitr(&self) -> bool {
        true
    }

    async fn pitr_restore(&self, config: &PitrConfig) -> Result<RestoreResult, DataSourceError> {
        let start = Instant::now();
        info!(
            "Starting PITR restore to {} from backup {}",
            config.target_time, config.base_backup_id
        );

        let backup_dir = config.backup_dir.clone().ok_or_else(|| {
            DataSourceError::Configuration("backup_dir is required for PITR".to_string())
        })?;

        // Create a minimal config for the manager
        let pg_config = PostgresConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            user: "postgres".to_string(),
            password: None,
            ssl_mode: None,
            maintenance_db: None,
            ssh_host: None,
            ssh_user: None,
            ssh_port: None,
            ssh_password: None,
            ssh_key_path: None,
            ssh_local_port: None,
            ssh_remote_port: None,
        };

        let mut manager = PostgresManager::new(pg_config, backup_dir)
            .map_err(Self::convert_error)?;

        let restore = manager
            .restore_point_in_time(
                &config.base_backup_id,
                config.target_dir.clone(),
                config.target_time,
            )
            .await
            .map_err(Self::convert_error)?;

        let duration = start.elapsed();

        let status = match restore.status {
            crate::common::RestoreStatus::Completed => RestoreStatus::Completed,
            crate::common::RestoreStatus::Failed => RestoreStatus::Failed,
            crate::common::RestoreStatus::InProgress => RestoreStatus::InProgress,
        };

        info!(
            "PITR restore {} completed in {:?}",
            restore.id, duration
        );

        Ok(RestoreResult {
            restore_id: restore.id,
            backup_id: config.base_backup_id,
            status,
            restore_path: config.target_dir.clone(),
            duration,
            service_restarted: config.auto_restart,
            target_time: Some(config.target_time),
            error_message: restore.error_message,
        })
    }

    async fn status(&self, config: &StatusConfig) -> Result<DataSourceStatus, DataSourceError> {
        info!(
            "Getting status for PostgreSQL at {}:{}",
            config.connection.host, config.connection.port
        );

        let pg_config = Self::to_postgres_config(&config.connection, config.ssh_tunnel.as_ref());
        let conn_string = pg_config.connection_string();

        let (client, connection) = match tokio_postgres::connect(&conn_string, tokio_postgres::NoTls).await {
            Ok(result) => result,
            Err(e) => {
                return Ok(DataSourceStatus {
                    connected: false,
                    server_version: None,
                    accepting_connections: false,
                    state: Some("disconnected".to_string()),
                    active_connections: None,
                    database_size_bytes: None,
                    extra: {
                        let mut extra = HashMap::new();
                        extra.insert("error".to_string(), serde_json::Value::String(e.to_string()));
                        extra
                    },
                });
            }
        };

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("PostgreSQL connection error: {}", e);
            }
        });

        // Get server version
        let version_row = client
            .query_one("SHOW server_version", &[])
            .await
            .map_err(|e| DataSourceError::Connection(e.to_string()))?;
        let server_version: String = version_row.get(0);

        // Check if in recovery (replica)
        let is_recovery_row = client
            .query_one("SELECT pg_is_in_recovery()", &[])
            .await
            .map_err(|e| DataSourceError::Connection(e.to_string()))?;
        let is_recovery: bool = is_recovery_row.get(0);

        let state = if is_recovery { "replica" } else { "primary" };

        let mut extra = HashMap::new();
        extra.insert("is_replica".to_string(), serde_json::Value::Bool(is_recovery));

        // Get active connections if requested
        let active_connections = if config.include_metrics {
            let conn_row = client
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE state = 'active'",
                    &[],
                )
                .await
                .ok();
            conn_row.map(|row| row.get::<_, i64>(0) as u32)
        } else {
            None
        };

        // Get database size if requested
        let database_size_bytes = if config.include_metrics {
            if let Some(db) = &config.connection.database {
                let size_row = client
                    .query_one(
                        "SELECT pg_database_size($1)",
                        &[db],
                    )
                    .await
                    .ok();
                size_row.map(|row| row.get::<_, i64>(0) as u64)
            } else {
                None
            }
        } else {
            None
        };

        // Get replication info if requested
        if config.include_replication && !is_recovery {
            let repl_rows = client
                .query(
                    "SELECT client_addr, state, sent_lsn, write_lsn, flush_lsn, replay_lsn 
                     FROM pg_stat_replication",
                    &[],
                )
                .await
                .ok();

            if let Some(rows) = repl_rows {
                let replicas: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|row| {
                        serde_json::json!({
                            "client_addr": row.get::<_, Option<std::net::IpAddr>>(0).map(|ip| ip.to_string()),
                            "state": row.get::<_, Option<String>>(1),
                        })
                    })
                    .collect();
                extra.insert("replicas".to_string(), serde_json::Value::Array(replicas));
            }
        }

        Ok(DataSourceStatus {
            connected: true,
            server_version: Some(server_version),
            accepting_connections: true,
            state: Some(state.to_string()),
            active_connections,
            database_size_bytes,
            extra,
        })
    }

    fn capabilities(&self) -> DataSourceCapabilities {
        DataSourceCapabilities {
            backup_types: vec![BackupType::Full, BackupType::Incremental, BackupType::Snapshot],
            supports_pitr: true,
            supports_incremental: true,
            supports_logical_backup: true,
            supports_physical_backup: true,
            supports_ssh_tunnel: true,
            supports_remote_storage: true,
            supports_ha: true,
            supports_encryption: true,
            supports_compression: true,
            custom: {
                let mut custom = HashMap::new();
                custom.insert("supports_wal_archiving".to_string(), true);
                custom.insert("supports_streaming_replication".to_string(), true);
                custom.insert("supports_logical_replication".to_string(), true);
                custom
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datasource_name() {
        let ds = PostgresDataSource::new();
        assert_eq!(ds.name(), "postgresql");
    }

    #[test]
    fn test_datasource_capabilities() {
        let ds = PostgresDataSource::new();
        let caps = ds.capabilities();
        
        assert!(caps.supports_pitr);
        assert!(caps.supports_incremental);
        assert!(caps.supports_logical_backup);
        assert!(caps.supports_physical_backup);
        assert!(caps.backup_types.contains(&BackupType::Full));
        assert!(caps.backup_types.contains(&BackupType::Incremental));
        assert!(caps.backup_types.contains(&BackupType::Snapshot));
    }

    #[test]
    fn test_supports_pitr() {
        let ds = PostgresDataSource::new();
        assert!(ds.supports_pitr());
    }
}
