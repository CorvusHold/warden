//! Core traits for the data source plugin system.
//!
//! This module defines the `DataSource` trait that all plugins must implement.

use async_trait::async_trait;

use super::config::{BackupConfig, DiscoverConfig, PitrConfig, RestoreConfig, StatusConfig};
use super::error::DataSourceError;
use super::types::{
    BackupFilter, BackupMetadata, BackupResult, DataSourceCapabilities, DataSourceStatus,
    DiscoverResult, RestoreResult,
};

/// Core trait that all data source plugins must implement.
///
/// This trait defines the contract between Warden core and data source plugins.
/// Each data source (PostgreSQL, MySQL, MongoDB, etc.) implements this trait
/// to provide backup, restore, and management capabilities.
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` to support concurrent operations
/// across multiple data sources, background scheduling, and AMQP command handling.
///
/// # Error Handling
///
/// All methods return `Result<T, DataSourceError>`. Implementations should:
/// - Use appropriate error variants for different failure modes
/// - Provide descriptive error messages
/// - Clean up resources on error (no orphaned files, connections, etc.)
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
/// use common::datasource::*;
///
/// pub struct PostgresDataSource {
///     // ... fields
/// }
///
/// #[async_trait]
/// impl DataSource for PostgresDataSource {
///     fn name(&self) -> &str {
///         "postgresql"
///     }
///
///     fn version(&self) -> &str {
///         env!("CARGO_PKG_VERSION")
///     }
///
///     fn description(&self) -> &str {
///         "PostgreSQL backup and restore with PITR support"
///     }
///
///     // ... implement other methods
/// }
/// ```
#[async_trait]
pub trait DataSource: Send + Sync {
    /// Returns the unique name of this data source.
    ///
    /// This name is used for:
    /// - CLI command routing (e.g., `warden postgresql backup`)
    /// - Plugin registry lookup
    /// - Configuration sections
    /// - Logging and metrics
    ///
    /// The name should be lowercase, alphanumeric, and use hyphens for
    /// multi-word names (e.g., "postgresql", "mysql", "mongodb").
    fn name(&self) -> &str;

    /// Returns the version of this plugin implementation.
    ///
    /// This should follow semantic versioning (e.g., "0.1.0").
    /// Typically this is the crate version: `env!("CARGO_PKG_VERSION")`.
    fn version(&self) -> &str;

    /// Returns a human-readable description of this data source.
    ///
    /// This is displayed in `warden plugins list` and `warden plugins info`.
    fn description(&self) -> &str;

    // === Discovery ===

    /// Discover and validate connection to the data source.
    ///
    /// This method tests connectivity and gathers information about the
    /// data source, including:
    /// - Server version
    /// - Available databases/schemas
    /// - Server metadata
    ///
    /// # Arguments
    ///
    /// * `config` - Discovery configuration including connection parameters
    ///
    /// # Returns
    ///
    /// * `Ok(DiscoverResult)` - Connection successful with server information
    /// * `Err(DataSourceError::Connection)` - Connection failed
    /// * `Err(DataSourceError::Authentication)` - Authentication failed
    async fn discover(&self, config: &DiscoverConfig) -> Result<DiscoverResult, DataSourceError>;

    // === Backup Operations ===

    /// Perform a backup operation.
    ///
    /// This method creates a backup of the data source according to the
    /// specified configuration. The backup may be:
    /// - Stored locally in `backup_dir`
    /// - Uploaded to remote storage (if configured)
    /// - Both local and remote
    ///
    /// # Arguments
    ///
    /// * `config` - Backup configuration including type, destination, etc.
    ///
    /// # Returns
    ///
    /// * `Ok(BackupResult)` - Backup completed successfully
    /// * `Err(DataSourceError::Backup)` - Backup operation failed
    /// * `Err(DataSourceError::Storage)` - Remote storage upload failed
    ///
    /// # Implementation Notes
    ///
    /// - Create backup metadata before starting
    /// - Update metadata on completion or failure
    /// - Clean up partial backups on error
    /// - Log progress for long-running operations
    async fn backup(&self, config: &BackupConfig) -> Result<BackupResult, DataSourceError>;

    /// List available backups with optional filtering.
    ///
    /// Returns metadata for backups matching the filter criteria.
    /// This may query local catalogs, remote storage, or both.
    ///
    /// # Arguments
    ///
    /// * `filter` - Filter criteria (type, status, date range, labels, etc.)
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<BackupMetadata>)` - List of matching backups
    /// * `Err(DataSourceError::Storage)` - Failed to query storage
    async fn list_backups(
        &self,
        filter: &BackupFilter,
    ) -> Result<Vec<BackupMetadata>, DataSourceError>;

    // === Restore Operations ===

    /// Restore from a backup.
    ///
    /// This method restores data from a backup to the target directory.
    /// Optionally, it can:
    /// - Download the backup from remote storage
    /// - Restart the data source service
    /// - Restore to a container
    ///
    /// # Arguments
    ///
    /// * `config` - Restore configuration including backup ID, target, etc.
    ///
    /// # Returns
    ///
    /// * `Ok(RestoreResult)` - Restore completed successfully
    /// * `Err(DataSourceError::BackupNotFound)` - Backup does not exist
    /// * `Err(DataSourceError::Restore)` - Restore operation failed
    ///
    /// # Safety
    ///
    /// This operation may overwrite existing data. Implementations should:
    /// - Verify the target directory is safe to write to
    /// - Optionally prompt for confirmation (via CLI)
    /// - Create backups of existing data if requested
    async fn restore(&self, config: &RestoreConfig) -> Result<RestoreResult, DataSourceError>;

    // === PITR (Optional) ===

    /// Returns whether this data source supports Point-in-Time Recovery.
    ///
    /// If this returns `false`, calling `pitr_restore` will return
    /// `Err(DataSourceError::PitrNotSupported)`.
    fn supports_pitr(&self) -> bool;

    /// Perform Point-in-Time Recovery.
    ///
    /// Restores the data source to a specific point in time using:
    /// - A base full backup
    /// - Transaction logs/WAL files
    ///
    /// # Arguments
    ///
    /// * `config` - PITR configuration including base backup, target time, etc.
    ///
    /// # Returns
    ///
    /// * `Ok(RestoreResult)` - PITR completed successfully
    /// * `Err(DataSourceError::PitrNotSupported)` - PITR not supported
    /// * `Err(DataSourceError::Pitr)` - PITR operation failed
    /// * `Err(DataSourceError::BackupNotFound)` - Base backup not found
    ///
    /// # Implementation Notes
    ///
    /// - Validate that the target time is within the recoverable range
    /// - Ensure all required WAL/transaction logs are available
    /// - Report progress for long-running operations
    async fn pitr_restore(&self, config: &PitrConfig) -> Result<RestoreResult, DataSourceError>;

    // === Status ===

    /// Get the current status of the data source.
    ///
    /// Returns information about the data source including:
    /// - Connection status
    /// - Server version
    /// - Database size
    /// - Replication status (if applicable)
    ///
    /// # Arguments
    ///
    /// * `config` - Status configuration including connection parameters
    ///
    /// # Returns
    ///
    /// * `Ok(DataSourceStatus)` - Status retrieved successfully
    /// * `Err(DataSourceError::Connection)` - Could not connect
    async fn status(&self, config: &StatusConfig) -> Result<DataSourceStatus, DataSourceError>;

    // === Capabilities ===

    /// Returns the capabilities of this data source.
    ///
    /// This is used by the CLI and registry to determine what operations
    /// are available for this data source.
    fn capabilities(&self) -> DataSourceCapabilities;
}

/// Extension trait for data sources that support High Availability operations.
///
/// This is an optional trait that plugins can implement to support
/// HA cluster management.
#[async_trait]
pub trait HaDataSource: DataSource {
    /// Get the current HA cluster status
    async fn ha_status(&self, config: &StatusConfig) -> Result<HaClusterStatus, DataSourceError>;

    /// Perform a switchover to a replica
    async fn switchover(
        &self,
        target_replica: &str,
        config: &StatusConfig,
    ) -> Result<SwitchoverResult, DataSourceError>;

    /// Perform a failover (forced promotion)
    async fn failover(
        &self,
        target_replica: &str,
        config: &StatusConfig,
    ) -> Result<FailoverResult, DataSourceError>;
}

/// HA cluster status
#[derive(Debug, Clone)]
pub struct HaClusterStatus {
    /// Primary node identifier
    pub primary: Option<String>,
    /// List of replica nodes
    pub replicas: Vec<ReplicaInfo>,
    /// Overall cluster health
    pub healthy: bool,
    /// Cluster state description
    pub state: String,
}

/// Information about a replica node
#[derive(Debug, Clone)]
pub struct ReplicaInfo {
    /// Replica identifier
    pub id: String,
    /// Replica host
    pub host: String,
    /// Replication lag in bytes
    pub lag_bytes: Option<u64>,
    /// Replication lag in seconds
    pub lag_seconds: Option<f64>,
    /// Whether the replica is in sync
    pub in_sync: bool,
    /// Replica state
    pub state: String,
}

/// Result of a switchover operation
#[derive(Debug, Clone)]
pub struct SwitchoverResult {
    /// Whether the switchover succeeded
    pub success: bool,
    /// New primary node
    pub new_primary: Option<String>,
    /// Duration of the switchover
    pub duration: std::time::Duration,
    /// Any warnings or notes
    pub warnings: Vec<String>,
}

/// Result of a failover operation
#[derive(Debug, Clone)]
pub struct FailoverResult {
    /// Whether the failover succeeded
    pub success: bool,
    /// New primary node
    pub new_primary: Option<String>,
    /// Duration of the failover
    pub duration: std::time::Duration,
    /// Data loss estimate (if any)
    pub data_loss_estimate: Option<String>,
    /// Any warnings or notes
    pub warnings: Vec<String>,
}
