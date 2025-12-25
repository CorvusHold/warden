//! Configuration types for data source operations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use super::types::BackupType;

/// Connection parameters for a data source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionParams {
    /// Host address
    pub host: String,
    /// Port number
    pub port: u16,
    /// Database/schema name
    pub database: Option<String>,
    /// Username for authentication
    pub user: Option<String>,
    /// Password for authentication (sensitive)
    #[serde(skip_serializing)]
    pub password: Option<String>,
    /// SSL/TLS mode
    pub ssl_mode: Option<String>,
    /// Additional connection options
    pub options: HashMap<String, String>,
}

impl ConnectionParams {
    /// Create new connection parameters
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            database: None,
            user: None,
            password: None,
            ssl_mode: None,
            options: HashMap::new(),
        }
    }

    /// Set the database name
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Set the username
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Set the password
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Set the SSL mode
    pub fn with_ssl_mode(mut self, ssl_mode: impl Into<String>) -> Self {
        self.ssl_mode = Some(ssl_mode.into());
        self
    }

    /// Add a custom option
    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }
}

impl Default for ConnectionParams {
    fn default() -> Self {
        Self::new("localhost", 5432)
    }
}

/// SSH tunnel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshTunnelConfig {
    /// SSH server host
    pub ssh_host: String,
    /// SSH server port
    pub ssh_port: u16,
    /// SSH username
    pub ssh_user: String,
    /// SSH password (if using password auth)
    #[serde(skip_serializing)]
    pub ssh_password: Option<String>,
    /// Path to SSH private key
    pub ssh_key_path: Option<PathBuf>,
    /// Local port for the tunnel
    pub local_port: Option<u16>,
    /// Remote host to tunnel to
    pub remote_host: String,
    /// Remote port to tunnel to
    pub remote_port: u16,
}

impl SshTunnelConfig {
    /// Create a new SSH tunnel configuration
    pub fn new(
        ssh_host: impl Into<String>,
        ssh_user: impl Into<String>,
        remote_host: impl Into<String>,
        remote_port: u16,
    ) -> Self {
        Self {
            ssh_host: ssh_host.into(),
            ssh_port: 22,
            ssh_user: ssh_user.into(),
            ssh_password: None,
            ssh_key_path: None,
            local_port: None,
            remote_host: remote_host.into(),
            remote_port,
        }
    }

    /// Set SSH port
    pub fn with_ssh_port(mut self, port: u16) -> Self {
        self.ssh_port = port;
        self
    }

    /// Set SSH password
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.ssh_password = Some(password.into());
        self
    }

    /// Set SSH key path
    pub fn with_key_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.ssh_key_path = Some(path.into());
        self
    }

    /// Set local port
    pub fn with_local_port(mut self, port: u16) -> Self {
        self.local_port = Some(port);
        self
    }
}

/// Remote storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Storage provider type (e.g., "s3", "minio", "gcs")
    pub provider: String,
    /// Bucket name
    pub bucket: String,
    /// Prefix/path within the bucket
    pub prefix: Option<String>,
    /// Region (for cloud providers)
    pub region: Option<String>,
    /// Custom endpoint URL (for MinIO, etc.)
    pub endpoint: Option<String>,
    /// Access key ID
    #[serde(skip_serializing)]
    pub access_key: Option<String>,
    /// Secret access key
    #[serde(skip_serializing)]
    pub secret_key: Option<String>,
    /// Whether to use path-style addressing
    pub path_style: bool,
}

impl StorageConfig {
    /// Create a new S3-compatible storage configuration
    pub fn s3(bucket: impl Into<String>, region: impl Into<String>) -> Self {
        Self {
            provider: "s3".to_string(),
            bucket: bucket.into(),
            prefix: None,
            region: Some(region.into()),
            endpoint: None,
            access_key: None,
            secret_key: None,
            path_style: false,
        }
    }

    /// Create a MinIO storage configuration
    pub fn minio(bucket: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            provider: "minio".to_string(),
            bucket: bucket.into(),
            prefix: None,
            region: None,
            endpoint: Some(endpoint.into()),
            access_key: None,
            secret_key: None,
            path_style: true,
        }
    }

    /// Set the prefix
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Set credentials
    pub fn with_credentials(
        mut self,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> Self {
        self.access_key = Some(access_key.into());
        self.secret_key = Some(secret_key.into());
        self
    }
}

/// Container configuration for restore operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// Container ID
    pub container_id: String,
    /// Container type (e.g., "docker", "podman")
    pub container_type: String,
    /// Data directory inside the container
    pub data_dir: Option<PathBuf>,
}

/// Configuration for discovery operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverConfig {
    /// Connection parameters
    pub connection: ConnectionParams,
    /// SSH tunnel configuration (optional)
    pub ssh_tunnel: Option<SshTunnelConfig>,
    /// Connection timeout
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
}

impl DiscoverConfig {
    /// Create a new discover configuration
    pub fn new(connection: ConnectionParams) -> Self {
        Self {
            connection,
            ssh_tunnel: None,
            timeout: Duration::from_secs(30),
        }
    }

    /// Set SSH tunnel configuration
    pub fn with_ssh_tunnel(mut self, tunnel: SshTunnelConfig) -> Self {
        self.ssh_tunnel = Some(tunnel);
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Configuration for backup operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Connection parameters
    pub connection: ConnectionParams,
    /// Type of backup to perform
    pub backup_type: BackupType,
    /// Local directory for backup storage
    pub backup_dir: PathBuf,
    /// Remote storage configuration (optional)
    pub remote_storage: Option<StorageConfig>,
    /// SSH tunnel configuration (optional)
    pub ssh_tunnel: Option<SshTunnelConfig>,
    /// Custom labels/tags for the backup
    pub labels: HashMap<String, String>,
    /// Whether to compress the backup
    pub compress: bool,
    /// Whether to encrypt the backup
    pub encrypt: bool,
    /// Encryption key ID (if encrypting)
    pub encryption_key_id: Option<String>,
}

impl BackupConfig {
    /// Create a new backup configuration
    pub fn new(
        connection: ConnectionParams,
        backup_type: BackupType,
        backup_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            connection,
            backup_type,
            backup_dir: backup_dir.into(),
            remote_storage: None,
            ssh_tunnel: None,
            labels: HashMap::new(),
            compress: true,
            encrypt: false,
            encryption_key_id: None,
        }
    }

    /// Set remote storage configuration
    pub fn with_remote_storage(mut self, storage: StorageConfig) -> Self {
        self.remote_storage = Some(storage);
        self
    }

    /// Set SSH tunnel configuration
    pub fn with_ssh_tunnel(mut self, tunnel: SshTunnelConfig) -> Self {
        self.ssh_tunnel = Some(tunnel);
        self
    }

    /// Add a label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Set compression
    pub fn with_compression(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Set encryption
    pub fn with_encryption(mut self, encrypt: bool, key_id: Option<String>) -> Self {
        self.encrypt = encrypt;
        self.encryption_key_id = key_id;
        self
    }
}

/// Configuration for restore operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreConfig {
    /// Backup to restore from
    pub backup_id: uuid::Uuid,
    /// Target directory for restored data
    pub target_dir: PathBuf,
    /// Connection parameters for target instance (optional)
    pub connection: Option<ConnectionParams>,
    /// Remote storage configuration (if backup is remote)
    pub remote_storage: Option<StorageConfig>,
    /// Local backup directory (if backup is local)
    pub backup_dir: Option<PathBuf>,
    /// Whether to auto-restart the service after restore
    pub auto_restart: bool,
    /// Container configuration (if restoring to container)
    pub container: Option<ContainerConfig>,
    /// SSH tunnel configuration (optional)
    pub ssh_tunnel: Option<SshTunnelConfig>,
}

impl RestoreConfig {
    /// Create a new restore configuration
    pub fn new(backup_id: uuid::Uuid, target_dir: impl Into<PathBuf>) -> Self {
        Self {
            backup_id,
            target_dir: target_dir.into(),
            connection: None,
            remote_storage: None,
            backup_dir: None,
            auto_restart: false,
            container: None,
            ssh_tunnel: None,
        }
    }

    /// Set connection parameters
    pub fn with_connection(mut self, connection: ConnectionParams) -> Self {
        self.connection = Some(connection);
        self
    }

    /// Set remote storage configuration
    pub fn with_remote_storage(mut self, storage: StorageConfig) -> Self {
        self.remote_storage = Some(storage);
        self
    }

    /// Set local backup directory
    pub fn with_backup_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.backup_dir = Some(dir.into());
        self
    }

    /// Set auto-restart
    pub fn with_auto_restart(mut self, auto_restart: bool) -> Self {
        self.auto_restart = auto_restart;
        self
    }

    /// Set container configuration
    pub fn with_container(mut self, container: ContainerConfig) -> Self {
        self.container = Some(container);
        self
    }
}

/// Configuration for Point-in-Time Recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitrConfig {
    /// Full backup to use as base
    pub base_backup_id: uuid::Uuid,
    /// Target point in time
    pub target_time: DateTime<Utc>,
    /// Target directory for restored data
    pub target_dir: PathBuf,
    /// Remote storage configuration
    pub remote_storage: Option<StorageConfig>,
    /// Local backup directory
    pub backup_dir: Option<PathBuf>,
    /// Auto-restart configuration
    pub auto_restart: bool,
    /// Container configuration (optional)
    pub container: Option<ContainerConfig>,
    /// SSH tunnel configuration (optional)
    pub ssh_tunnel: Option<SshTunnelConfig>,
}

impl PitrConfig {
    /// Create a new PITR configuration
    pub fn new(
        base_backup_id: uuid::Uuid,
        target_time: DateTime<Utc>,
        target_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            base_backup_id,
            target_time,
            target_dir: target_dir.into(),
            remote_storage: None,
            backup_dir: None,
            auto_restart: false,
            container: None,
            ssh_tunnel: None,
        }
    }

    /// Set remote storage configuration
    pub fn with_remote_storage(mut self, storage: StorageConfig) -> Self {
        self.remote_storage = Some(storage);
        self
    }

    /// Set local backup directory
    pub fn with_backup_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.backup_dir = Some(dir.into());
        self
    }

    /// Set auto-restart
    pub fn with_auto_restart(mut self, auto_restart: bool) -> Self {
        self.auto_restart = auto_restart;
        self
    }
}

/// Configuration for status queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusConfig {
    /// Connection parameters
    pub connection: ConnectionParams,
    /// SSH tunnel configuration (optional)
    pub ssh_tunnel: Option<SshTunnelConfig>,
    /// Whether to include detailed metrics
    pub include_metrics: bool,
    /// Whether to include replication status
    pub include_replication: bool,
}

impl StatusConfig {
    /// Create a new status configuration
    pub fn new(connection: ConnectionParams) -> Self {
        Self {
            connection,
            ssh_tunnel: None,
            include_metrics: false,
            include_replication: false,
        }
    }

    /// Set SSH tunnel configuration
    pub fn with_ssh_tunnel(mut self, tunnel: SshTunnelConfig) -> Self {
        self.ssh_tunnel = Some(tunnel);
        self
    }

    /// Include detailed metrics
    pub fn with_metrics(mut self) -> Self {
        self.include_metrics = true;
        self
    }

    /// Include replication status
    pub fn with_replication(mut self) -> Self {
        self.include_replication = true;
        self
    }
}

/// Serde helper for Duration
mod humantime_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}
