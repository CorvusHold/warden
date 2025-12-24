//! Migration tooling for existing PostgreSQL deployments.
//!
//! This module provides commands to help operators migrate existing PostgreSQL
//! deployments to Warden management, including:
//! - Discovery of existing databases
//! - Import of existing backups
//! - Configuration generation

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_yml;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use common::config::{
    Cluster, ClusterConfig, ConnectionConfig, Node, NodeRole, ProtectionGroup, SshConfig,
};
use common::schedule::{
    BackupSchedule, BackupTarget, BackupType, RetentionSchedule, ScheduleConfig, StorageProfile,
};

use crate::common::PostgresConfig;
use crate::tunnel_keeper::TunnelKeeper;

use storage::{
    BackupMetadata, BackupStatus as StorageBackupStatus, BackupType as StorageBackupType,
};

struct TunnelGuard {
    keeper: std::sync::Arc<tokio::sync::Mutex<TunnelKeeper>>,
    enabled: bool,
}

impl TunnelGuard {
    fn new(keeper: std::sync::Arc<tokio::sync::Mutex<TunnelKeeper>>) -> Self {
        Self {
            keeper,
            enabled: true,
        }
    }
}

impl Drop for TunnelGuard {
    /// Closes the SSH tunnel on drop if enabled.
    ///
    /// This is a best-effort cleanup mechanism that spawns an async task to close the tunnel.
    /// The async task may fail or be cancelled if:
    /// - The Tokio runtime is shutting down
    /// - The system is under heavy resource pressure
    /// - The tunnel was already explicitly closed before drop
    ///
    /// For guaranteed cleanup, explicitly call the cleanup method at lines 424-431
    /// before the scope ends or when done with the tunnel.
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }

        let keeper = self.keeper.clone();
        tokio::spawn(async move {
            let mut keeper = keeper.lock().await;
            let _ = keeper.close().await;
        });
    }
}

// ============================================================================
// Discovery Types
// ============================================================================

/// Result of discovering an existing PostgreSQL instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    /// PostgreSQL server version
    pub version: String,
    /// Server hostname
    pub host: String,
    /// Server port
    pub port: u16,
    /// List of databases with their sizes
    pub databases: Vec<DatabaseInfo>,
    /// Replication status
    pub replication: ReplicationInfo,
    /// Detected backup configuration (if any)
    pub backup_config: Option<DetectedBackupConfig>,
    /// Recommended Warden configuration
    pub recommendations: Vec<String>,
    /// Timestamp of discovery
    pub discovered_at: DateTime<Utc>,
}

/// Information about a discovered database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseInfo {
    /// Database name
    pub name: String,
    /// Database size in bytes
    pub size_bytes: u64,
    /// Database owner
    pub owner: String,
    /// Encoding
    pub encoding: String,
    /// Number of tables (approximate)
    pub table_count: Option<i64>,
    /// Whether the database is a template
    pub is_template: bool,
    /// Whether connections are allowed
    pub allow_connections: bool,
}

/// Replication status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationInfo {
    /// Node role: primary, replica, or standalone
    pub role: String,
    /// Whether WAL archiving is enabled
    pub wal_archiving_enabled: bool,
    /// Archive command (if configured)
    pub archive_command: Option<String>,
    /// Connected replicas (if primary)
    pub replicas: Vec<ReplicaInfo>,
    /// Primary connection info (if replica)
    pub primary_conninfo: Option<String>,
    /// Current WAL position
    pub current_wal_lsn: Option<String>,
    /// Replication lag (if replica)
    pub replication_lag_bytes: Option<i64>,
}

/// Information about a connected replica
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaInfo {
    /// Application name
    pub application_name: String,
    /// Client address
    pub client_addr: Option<String>,
    /// Replication state
    pub state: String,
    /// Sent LSN
    pub sent_lsn: Option<String>,
    /// Write LSN
    pub write_lsn: Option<String>,
    /// Flush LSN
    pub flush_lsn: Option<String>,
    /// Replay LSN
    pub replay_lsn: Option<String>,
}

/// Detected backup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedBackupConfig {
    /// Whether pg_dump backups are detected
    pub has_pg_dump: bool,
    /// Whether pg_basebackup is configured
    pub has_pg_basebackup: bool,
    /// Backup directory (if detected)
    pub backup_directory: Option<String>,
    /// Archive directory (if WAL archiving is enabled)
    pub archive_directory: Option<String>,
    /// Detected backup schedule (if any)
    pub schedule: Option<String>,
}

// ============================================================================
// Config Generation Types
// ============================================================================

/// Options for generating Warden configuration
#[derive(Debug, Clone)]
pub struct GenerateConfigOptions {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    pub cluster_name: Option<String>,
    pub tenant: Option<String>,
    pub output_path: Option<PathBuf>,
    pub interactive: bool,
    pub ssh: SshOptions,
}

/// SSH connection options
#[derive(Debug, Clone, Default)]
pub struct SshOptions {
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
    pub key_path: Option<String>,
    pub local_port: Option<u16>,
    pub remote_port: Option<u16>,
}

/// Generated configuration files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedConfig {
    /// Cluster configuration YAML
    pub cluster_yaml: String,
    /// Suggested schedule configuration
    pub schedule_config: String,
    /// Suggested retention policy
    pub retention_policy: String,
    /// Path where config was written (if output was specified)
    pub output_path: Option<PathBuf>,
}

// ============================================================================
// Backup Import Types
// ============================================================================

/// Type of backup being imported
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportBackupType {
    /// pg_dump output (.sql, .dump, .tar)
    PgDump,
    /// pg_basebackup directory
    PgBasebackup,
    /// Custom format (e.g., from another backup tool)
    Custom,
}

impl std::str::FromStr for ImportBackupType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pg_dump" | "pgdump" | "dump" => Ok(ImportBackupType::PgDump),
            "pg_basebackup" | "pgbasebackup" | "basebackup" | "base" => {
                Ok(ImportBackupType::PgBasebackup)
            }
            "custom" => Ok(ImportBackupType::Custom),
            _ => Err(anyhow!(
                "Unknown backup type: {}. Valid types: pg_dump, pg_basebackup, custom",
                s
            )),
        }
    }
}

impl std::fmt::Display for ImportBackupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportBackupType::PgDump => write!(f, "pg_dump"),
            ImportBackupType::PgBasebackup => write!(f, "pg_basebackup"),
            ImportBackupType::Custom => write!(f, "custom"),
        }
    }
}

/// Options for importing a backup
#[derive(Debug, Clone)]
pub struct ImportBackupOptions {
    /// Source path or URL
    pub source: String,
    /// Type of backup
    pub backup_type: ImportBackupType,
    /// Database name
    pub database: String,
    /// Tenant identifier
    pub tenant: Option<String>,
    /// Cluster identifier
    pub cluster: Option<String>,
    /// Storage profile name
    pub storage_profile: Option<String>,
    /// Local backup directory
    pub backup_dir: PathBuf,
    /// Storage options for remote upload
    pub storage: Option<StorageOptions>,
}

/// Storage options for backup operations
#[derive(Debug, Clone, Default)]
pub struct StorageOptions {
    pub enabled: bool,
    pub provider: Option<String>,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

/// Result of importing a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    /// Generated backup ID
    pub backup_id: String,
    /// Local path where backup was stored
    pub local_path: PathBuf,
    /// Remote path (if uploaded)
    pub remote_path: Option<String>,
    /// Database name
    pub database: String,
    /// Backup type
    pub backup_type: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// Import timestamp
    pub imported_at: DateTime<Utc>,
    /// Original source path
    pub original_source: String,
}

// ============================================================================
// Discovery Implementation
// ============================================================================

/// Discover an existing PostgreSQL instance
pub async fn discover(
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    ssh: SshOptions,
) -> Result<DiscoveryResult> {
    info!(
        "[discover] Starting discovery of PostgreSQL at {}:{}",
        host, port
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
        host: effective_host.clone(),
        port: effective_port,
        database: "postgres".to_string(),
        user: user.clone(),
        password: password.clone(),
        ssl_mode: ssl_mode.clone(),
        maintenance_db: Some("postgres".to_string()),
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };

    let mut tunnel_guard = if config.ssh_host.is_some() {
        info!("[discover] Setting up SSH tunnel...");
        let keeper_instance = TunnelKeeper::instance().await;
        {
            let mut keeper = keeper_instance.lock().await;
            if let Err(e) = keeper.setup(&config).await {
                error!("[discover] Failed to setup SSH tunnel: {e}");
                return Err(anyhow!("SSH tunnel setup failed: {}", e));
            }
        }
        info!("[discover] SSH tunnel established successfully");
        Some(TunnelGuard::new(keeper_instance))
    } else {
        None
    };

    // Connect to PostgreSQL
    let conn_string = config.connection_string();
    let ssl_mode = config.ssl_mode.as_deref().map(|s| s.trim().to_lowercase());
    let use_tls = !matches!(ssl_mode.as_deref(), None | Some("") | Some("disable"));

    let client = if use_tls {
        if matches!(ssl_mode.as_deref(), Some("allow") | Some("prefer")) {
            log::warn!(
                "[discover] ssl_mode '{}' requested; using TLS (no non-TLS fallback)",
                ssl_mode.as_deref().unwrap_or("")
            );
        }

        let tls = native_tls::TlsConnector::builder()
            .build()
            .context("Failed to build TLS connector")?;
        let tls = postgres_native_tls::MakeTlsConnector::new(tls);

        let (client, connection) = tokio_postgres::connect(&conn_string, tls)
            .await
            .context("Failed to connect to PostgreSQL")?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("[discover] Connection error: {}", e);
            }
        });

        client
    } else {
        let (client, connection) = tokio_postgres::connect(&conn_string, tokio_postgres::NoTls)
            .await
            .context("Failed to connect to PostgreSQL")?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("[discover] Connection error: {}", e);
            }
        });

        client
    };

    // Get PostgreSQL version
    let version = get_pg_version(&client).await?;
    info!("[discover] PostgreSQL version: {}", version);

    // Get database list
    let databases = get_database_list(&client).await?;
    info!("[discover] Found {} databases", databases.len());

    // Get replication status
    let replication = get_replication_info(&client).await?;
    info!("[discover] Replication role: {}", replication.role);

    // Try to detect backup configuration
    let backup_config = detect_backup_config(&client).await.ok();

    // Generate recommendations
    let recommendations = generate_recommendations(&databases, &replication, &backup_config);

    // Close SSH tunnel if it was opened
    if let Some(guard) = tunnel_guard.as_mut() {
        // We're doing an explicit, awaited close here, so disable the Drop handler.
        guard.enabled = false;

        let mut keeper = guard.keeper.lock().await;
        let _ = keeper.close().await;
        info!("[discover] SSH tunnel closed");
    }

    Ok(DiscoveryResult {
        version,
        host,
        port,
        databases,
        replication,
        backup_config,
        recommendations,
        discovered_at: Utc::now(),
    })
}

async fn get_pg_version(client: &tokio_postgres::Client) -> Result<String> {
    let row = client
        .query_one("SELECT version()", &[])
        .await
        .context("Failed to get PostgreSQL version")?;
    let version: String = row.get(0);
    Ok(version)
}

async fn get_database_list(client: &tokio_postgres::Client) -> Result<Vec<DatabaseInfo>> {
    let rows = client
        .query(
            r#"
            SELECT 
                d.datname as name,
                pg_database_size(d.datname) as size_bytes,
                r.rolname as owner,
                pg_encoding_to_char(d.encoding) as encoding,
                d.datistemplate as is_template,
                d.datallowconn as allow_connections
            FROM pg_database d
            JOIN pg_roles r ON d.datdba = r.oid
            WHERE d.datname NOT IN ('template0')
            ORDER BY d.datname
            "#,
            &[],
        )
        .await
        .context("Failed to list databases")?;

    let mut databases = Vec::new();
    for row in rows {
        let name: String = row.get("name");
        let size_bytes: i64 = row.get("size_bytes");
        let owner: String = row.get("owner");
        let encoding: String = row.get("encoding");
        let is_template: bool = row.get("is_template");
        let allow_connections: bool = row.get("allow_connections");

        // Try to get table count for non-template databases
        let table_count = if !is_template && allow_connections {
            match client
                .query_one(
                    r#"
                    SELECT COUNT(*) as table_count
                    FROM information_schema.tables
                    WHERE table_catalog = $1
                      AND table_schema NOT IN ('pg_catalog', 'information_schema')
                    "#,
                    &[&name],
                )
                .await
            {
                Ok(count_row) => Some(count_row.get::<_, i64>("table_count")),
                Err(_) => None, // Silently ignore errors for individual databases
            }
        } else {
            None
        };

        databases.push(DatabaseInfo {
            name,
            size_bytes: size_bytes as u64,
            owner,
            encoding,
            table_count,
            is_template,
            allow_connections,
        });
    }

    Ok(databases)
}

async fn get_replication_info(client: &tokio_postgres::Client) -> Result<ReplicationInfo> {
    // Check if this is a replica
    let is_replica_row = client
        .query_one("SELECT pg_is_in_recovery()", &[])
        .await
        .context("Failed to check recovery status")?;
    let is_replica: bool = is_replica_row.get(0);

    // Get current WAL position
    let wal_lsn = if is_replica {
        let row = client
            .query_one("SELECT pg_last_wal_receive_lsn()::text", &[])
            .await
            .ok();
        row.and_then(|r| r.get::<_, Option<String>>(0))
    } else {
        let row = client
            .query_one("SELECT pg_current_wal_lsn()::text", &[])
            .await
            .ok();
        row.and_then(|r| r.get::<_, Option<String>>(0))
    };

    // Check WAL archiving status
    let archive_row = client
        .query_one(
            "SELECT setting FROM pg_settings WHERE name = 'archive_mode'",
            &[],
        )
        .await
        .ok();
    let archive_mode = archive_row
        .map(|r| r.get::<_, String>(0))
        .unwrap_or_default();
    let wal_archiving_enabled = archive_mode == "on" || archive_mode == "always";

    // Get archive command if archiving is enabled
    let archive_command = if wal_archiving_enabled {
        let row = client
            .query_one(
                "SELECT setting FROM pg_settings WHERE name = 'archive_command'",
                &[],
            )
            .await
            .ok();
        row.map(|r| r.get::<_, String>(0))
    } else {
        None
    };

    // Get replica info if this is a primary
    let replicas = if !is_replica {
        get_replica_list(client).await.unwrap_or_default()
    } else {
        Vec::new()
    };

    // Get primary connection info if this is a replica
    let primary_conninfo = if is_replica {
        let row = client
            .query_one(
                "SELECT setting FROM pg_settings WHERE name = 'primary_conninfo'",
                &[],
            )
            .await
            .ok();
        row.map(|r| r.get::<_, String>(0)).filter(|s| !s.is_empty())
    } else {
        None
    };

    // Calculate replication lag if replica
    let replication_lag_bytes = if is_replica {
        let row = client
            .query_one(
                r#"
                SELECT pg_wal_lsn_diff(
                    pg_last_wal_receive_lsn(),
                    pg_last_wal_replay_lsn()
                )::bigint
                "#,
                &[],
            )
            .await
            .ok();
        row.and_then(|r| r.get::<_, Option<i64>>(0))
    } else {
        None
    };

    let role = if is_replica {
        if replicas.is_empty() {
            "replica".to_string()
        } else {
            "cascading_replica".to_string()
        }
    } else if !replicas.is_empty() {
        "primary".to_string()
    } else {
        "standalone".to_string()
    };

    Ok(ReplicationInfo {
        role,
        wal_archiving_enabled,
        archive_command,
        replicas,
        primary_conninfo,
        current_wal_lsn: wal_lsn,
        replication_lag_bytes,
    })
}

async fn get_replica_list(client: &tokio_postgres::Client) -> Result<Vec<ReplicaInfo>> {
    let rows = client
        .query(
            r#"
            SELECT 
                application_name,
                client_addr::text,
                state,
                sent_lsn::text,
                write_lsn::text,
                flush_lsn::text,
                replay_lsn::text
            FROM pg_stat_replication
            "#,
            &[],
        )
        .await
        .context("Failed to get replication stats")?;

    let mut replicas = Vec::new();
    for row in rows {
        replicas.push(ReplicaInfo {
            application_name: row.get("application_name"),
            client_addr: row.get("client_addr"),
            state: row.get("state"),
            sent_lsn: row.get("sent_lsn"),
            write_lsn: row.get("write_lsn"),
            flush_lsn: row.get("flush_lsn"),
            replay_lsn: row.get("replay_lsn"),
        });
    }

    Ok(replicas)
}

async fn detect_backup_config(client: &tokio_postgres::Client) -> Result<DetectedBackupConfig> {
    // Check archive settings
    let archive_row = client
        .query_one(
            "SELECT setting FROM pg_settings WHERE name = 'archive_command'",
            &[],
        )
        .await
        .ok();
    let archive_command = archive_row.map(|r| r.get::<_, String>(0));

    // Try to extract archive directory from archive_command
    let archive_directory = archive_command.as_ref().and_then(|cmd| {
        // Common patterns: cp %p /path/to/archive/%f
        if let Some(pos) = cmd.find(" /") {
            let rest = &cmd[pos + 1..];
            if let Some(end) = rest.find("/%f") {
                return Some(rest[..end].to_string());
            }
        }
        None
    });

    let has_pg_basebackup = archive_command.is_some();

    Ok(DetectedBackupConfig {
        has_pg_dump: false, // Can't easily detect this from server side
        has_pg_basebackup,
        backup_directory: None,
        archive_directory,
        schedule: None,
    })
}

fn generate_recommendations(
    databases: &[DatabaseInfo],
    replication: &ReplicationInfo,
    backup_config: &Option<DetectedBackupConfig>,
) -> Vec<String> {
    let mut recommendations = Vec::new();

    // Database-based recommendations
    let user_dbs: Vec<_> = databases
        .iter()
        .filter(|d| !d.is_template && d.name != "postgres")
        .collect();

    if user_dbs.is_empty() {
        recommendations.push(
            "No user databases found. Create databases before configuring backups.".to_string(),
        );
    } else {
        let total_size: u64 = user_dbs.iter().map(|d| d.size_bytes).sum();
        let size_gb = total_size as f64 / 1024.0 / 1024.0 / 1024.0;

        if size_gb > 100.0 {
            recommendations.push(format!(
                "Large database footprint ({:.1} GB). Consider incremental backups and parallel pg_dump.",
                size_gb
            ));
        }

        recommendations.push(format!(
            "Configure backup schedules for {} user database(s): {}",
            user_dbs.len(),
            user_dbs
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // Replication-based recommendations
    match replication.role.as_str() {
        "primary" => {
            recommendations.push(format!(
                "Primary node with {} replica(s). Consider backing up from a replica to reduce primary load.",
                replication.replicas.len()
            ));
        }
        "replica" => {
            recommendations.push(
                "This is a replica node. Ideal for running backups without impacting the primary."
                    .to_string(),
            );
            if let Some(lag) = replication.replication_lag_bytes {
                if lag > 1024 * 1024 {
                    recommendations.push(format!(
                        "Warning: Replication lag is {} bytes. Ensure replica is caught up before backup.",
                        lag
                    ));
                }
            }
        }
        "standalone" => {
            recommendations.push(
                "Standalone instance (no replication). Consider setting up a replica for HA."
                    .to_string(),
            );
        }
        _ => {}
    }

    // WAL archiving recommendations
    if !replication.wal_archiving_enabled {
        recommendations.push(
            "WAL archiving is not enabled. Enable it for point-in-time recovery (PITR) capability."
                .to_string(),
        );
    } else {
        recommendations
            .push("WAL archiving is enabled. PITR will be available for backups.".to_string());
    }

    // Backup configuration recommendations
    if let Some(config) = backup_config {
        if config.archive_directory.is_some() {
            recommendations.push(
                "Existing WAL archive detected. Consider importing existing WAL files.".to_string(),
            );
        }
    }

    recommendations
}

// ============================================================================
// Config Generation Implementation
// ============================================================================

/// Generate Warden configuration from discovered PostgreSQL instance
pub async fn generate_config(options: GenerateConfigOptions) -> Result<GeneratedConfig> {
    info!(
        "[generate-config] Generating configuration for {}:{}",
        options.host, options.port
    );

    // First, discover the instance
    let discovery = discover(
        options.host.clone(),
        options.port,
        options.user.clone(),
        options.password.clone(),
        options.ssl_mode.clone(),
        options.ssh.clone(),
    )
    .await?;

    // Generate cluster name
    let cluster_name = options
        .cluster_name
        .clone()
        .unwrap_or_else(|| format!("{}-cluster", options.host.replace('.', "-")));

    // Generate cluster.yaml
    let cluster_yaml = generate_cluster_yaml(&discovery, &cluster_name, &options)?;

    // Generate schedule configuration
    let schedule_config = generate_schedule_config(&discovery, &cluster_name)?;

    // Generate retention policy
    let retention_policy = generate_retention_policy(&discovery)?;

    // Write to file if output path specified
    let output_path = if let Some(ref path) = options.output_path {
        let cluster_path = path.join("cluster.yaml");
        std::fs::create_dir_all(path)?;
        std::fs::write(&cluster_path, &cluster_yaml)?;

        let schedule_path = path.join("schedule.yaml");
        std::fs::write(&schedule_path, &schedule_config)?;

        let retention_path = path.join("retention-policy.json");
        std::fs::write(&retention_path, &retention_policy)?;

        info!("[generate-config] Configuration written to {:?}", path);
        Some(path.clone())
    } else {
        None
    };

    Ok(GeneratedConfig {
        cluster_yaml,
        schedule_config,
        retention_policy,
        output_path,
    })
}

fn generate_cluster_yaml(
    discovery: &DiscoveryResult,
    cluster_name: &str,
    options: &GenerateConfigOptions,
) -> Result<String> {
    let cluster_id = cluster_name.to_lowercase().replace(' ', "-");
    let node_id = format!("{}-node", cluster_id);

    let node_role = match discovery.replication.role.as_str() {
        "primary" => NodeRole::Primary,
        "replica" | "cascading_replica" => NodeRole::Replica,
        _ => NodeRole::Unknown,
    };

    let user_dbs: Vec<String> = discovery
        .databases
        .iter()
        .filter(|d| !d.is_template && d.name != "postgres" && d.allow_connections)
        .map(|d| d.name.clone())
        .collect();

    let mut cluster_labels = HashMap::new();
    cluster_labels.insert("discovered".to_string(), "true".to_string());
    cluster_labels.insert(
        "pg_version".to_string(),
        discovery
            .version
            .split_whitespace()
            .nth(1)
            .unwrap_or("unknown")
            .to_string(),
    );

    let mut node_labels = HashMap::new();
    node_labels.insert("discovered".to_string(), "true".to_string());

    let ssh = if options.ssh.host.is_some() {
        Some(SshConfig {
            host: options.ssh.host.clone().unwrap_or_default(),
            user: options.ssh.user.clone(),
            port: options.ssh.port.unwrap_or(22),
            key_path: Some("/etc/warden/ssh/key".to_string()),
            password_env: None,
        })
    } else {
        None
    };

    let node = Node {
        id: node_id,
        cluster_id: cluster_id.clone(),
        host: discovery.host.clone(),
        port: discovery.port,
        role: node_role,
        labels: node_labels,
        connection: Some(ConnectionConfig {
            user: Some(options.user.clone()),
            database: Some("postgres".to_string()),
            ssl_mode: options.ssl_mode.clone(),
            password_env: None,
        }),
        ssh,
    };

    let cluster = Cluster {
        id: cluster_id.clone(),
        name: Some(cluster_name.to_string()),
        tenant: None,
        environment: Some("production".to_string()),
        labels: cluster_labels,
    };

    let mut protection_groups = Vec::new();
    if !user_dbs.is_empty() {
        let preferred_source_role = match node_role {
            NodeRole::Primary => Some(NodeRole::Replica),
            _ => Some(NodeRole::Primary),
        };

        let mut labels = HashMap::new();
        labels.insert("backup_priority".to_string(), "high".to_string());

        protection_groups.push(ProtectionGroup {
            id: format!("{}-databases", cluster_id),
            name: Some(format!("{} Databases", cluster_name)),
            cluster_id: cluster_id.clone(),
            databases: user_dbs,
            preferred_source_role,
            labels,
        });
    }

    let config = ClusterConfig {
        version: "1".to_string(),
        default_tenant: options.tenant.clone(),
        clusters: vec![cluster],
        nodes: vec![node],
        protection_groups,
    };

    let yaml = serde_yml::to_string(&config).context("Failed to serialize cluster config")?;
    Ok(yaml)
}

fn generate_schedule_config(discovery: &DiscoveryResult, cluster_name: &str) -> Result<String> {
    let cluster_id = cluster_name.to_lowercase().replace(' ', "-");

    // Get user databases
    let user_dbs: Vec<_> = discovery
        .databases
        .iter()
        .filter(|d| !d.is_template && d.name != "postgres" && d.allow_connections)
        .collect();

    let total_size: u64 = user_dbs.iter().map(|d| d.size_bytes).sum();
    let size_gb = total_size as f64 / 1024.0 / 1024.0 / 1024.0;

    // Adjust schedule based on database size
    let (backup_cron, retention_cron) = if size_gb > 100.0 {
        // Large databases: weekly full, daily incremental
        ("0 2 * * 0", "0 4 * * 0") // Sunday 2 AM backup, 4 AM retention
    } else if size_gb > 10.0 {
        // Medium databases: daily backup
        ("0 2 * * *", "0 4 * * *") // Daily 2 AM backup, 4 AM retention
    } else {
        // Small databases: twice daily
        ("0 2,14 * * *", "0 4 * * *") // 2 AM and 2 PM backup, 4 AM retention
    };

    let profile = StorageProfile {
        name: "default-s3".to_string(),
        provider: "s3".to_string(),
        bucket: "warden-backups".to_string(),
        prefix: Some(format!("{}/", cluster_id)),
        region: Some("us-east-1".to_string()),
        endpoint: None,
        access_key: Some("env:AWS_ACCESS_KEY_ID".to_string()),
        secret_key: Some("env:AWS_SECRET_ACCESS_KEY".to_string()),
        encryption: None,
    };

    let mut backup_labels = HashMap::new();
    backup_labels.insert("cluster".to_string(), cluster_id.clone());
    backup_labels.insert("type".to_string(), "scheduled".to_string());

    let backup = BackupSchedule {
        id: format!("{}-daily", cluster_id),
        name: Some(format!("{} Daily Backup", cluster_name)),
        cron: backup_cron.to_string(),
        target: BackupTarget::Database {
            host: discovery.host.clone(),
            port: Some(discovery.port),
            database: "postgres".to_string(),
            user: Some("postgres".to_string()),
        },
        backup_type: BackupType::Snapshot,
        storage_profile: Some("default-s3".to_string()),
        enabled: true,
        labels: backup_labels,
        backup_dir: None,
        encryption: None,
    };

    let retention = RetentionSchedule {
        id: format!("{}-retention", cluster_id),
        name: Some(format!("{} Retention Cleanup", cluster_name)),
        cron: retention_cron.to_string(),
        policy_file: Some("./retention-policy.json".to_string()),
        policy: None,
        storage_profile: Some("default-s3".to_string()),
        enabled: true,
        apply: false,
        backup_dir: None,
    };

    let schedule_config = ScheduleConfig {
        backups: vec![backup],
        retention: vec![retention],
        storage_profiles: vec![profile],
        default_backup_dir: Some("./backups".to_string()),
    };

    #[derive(Serialize)]
    struct ScheduleFile {
        schedules: ScheduleConfig,
    }

    let yaml = serde_yml::to_string(&ScheduleFile {
        schedules: schedule_config,
    })
    .context("Failed to serialize schedule config")?;
    Ok(yaml)
}

fn generate_retention_policy(discovery: &DiscoveryResult) -> Result<String> {
    // Get total database size for sizing recommendations
    let user_dbs: Vec<_> = discovery
        .databases
        .iter()
        .filter(|d| !d.is_template && d.name != "postgres")
        .collect();

    let total_size: u64 = user_dbs.iter().map(|d| d.size_bytes).sum();
    let size_gb = total_size as f64 / 1024.0 / 1024.0 / 1024.0;

    // Adjust retention based on database size
    let (daily_count, weekly_count, monthly_count) = if size_gb > 100.0 {
        (7, 4, 3) // Shorter retention for large DBs
    } else if size_gb > 10.0 {
        (14, 8, 6) // Medium retention
    } else {
        (30, 12, 12) // Longer retention for small DBs
    };

    // Build retention policy using serde_json for type-safe JSON generation
    let policy = json!({
        "version": "1",
        "description": format!("Retention policy generated from discovery (total size: {:.2} GB)", size_gb),
        "rules": [
            {
                "name": "daily",
                "keep_count": daily_count,
                "keep_days": Value::Null,
                "backup_types": ["snapshot", "full"],
                "labels": {},
                "description": format!("Keep last {} daily backups", daily_count)
            },
            {
                "name": "weekly",
                "keep_count": weekly_count,
                "keep_days": Value::Null,
                "backup_types": ["snapshot", "full"],
                "labels": {"weekly": "true"},
                "description": format!("Keep last {} weekly backups", weekly_count)
            },
            {
                "name": "monthly",
                "keep_count": monthly_count,
                "keep_days": Value::Null,
                "backup_types": ["snapshot", "full"],
                "labels": {"monthly": "true"},
                "description": format!("Keep last {} monthly backups", monthly_count)
            }
        ],
        "default_action": "delete",
        "pitr_retention_days": 7,
        "wal_retention_days": 7
    });

    let json = serde_json::to_string_pretty(&policy)
        .context("Failed to serialize retention policy to JSON")?;

    Ok(json)
}

// ============================================================================
// Backup Import Implementation
// ============================================================================

/// Import an existing backup into Warden's catalog
pub async fn import_backup(options: ImportBackupOptions) -> Result<ImportResult> {
    info!(
        "[import-backup] Importing {} backup from {}",
        options.backup_type, options.source
    );

    // Validate source exists
    let source_path = PathBuf::from(&options.source);
    let is_local = source_path.exists();
    let is_s3 = options.source.starts_with("s3://");

    if !is_local && !is_s3 {
        return Err(anyhow!(
            "Source not found: {}. Provide a local path or S3 URL (s3://bucket/key)",
            options.source
        ));
    }

    // Generate backup ID
    let backup_id = Uuid::new_v4().to_string();
    let timestamp = Utc::now();

    // Create backup directory structure
    let backup_dir = options.backup_dir.join(&backup_id);
    std::fs::create_dir_all(&backup_dir)?;

    // Import based on source type
    let (local_path, size_bytes) = if is_local {
        import_local_backup(&source_path, &backup_dir, &options).await?
    } else if is_s3 {
        import_s3_backup(&options.source, &backup_dir, &options).await?
    } else {
        return Err(anyhow!("Unsupported source type"));
    };

    // Create metadata
    let metadata = create_import_metadata(
        &backup_id,
        &options.database,
        &options.backup_type,
        size_bytes,
        timestamp,
        &options.source,
        &options.tenant,
        &options.cluster,
    );

    // Write metadata file
    let metadata_path = backup_dir.join("backup_metadata.json");
    let metadata_json = serde_json::to_string_pretty(&metadata)?;
    std::fs::write(&metadata_path, &metadata_json)?;
    info!("[import-backup] Metadata written to {:?}", metadata_path);

    // Upload to remote storage if configured
    let remote_path = if let Some(ref storage) = options.storage {
        if storage.enabled {
            upload_imported_backup(&backup_dir, &backup_id, &options, storage).await?
        } else {
            None
        }
    } else {
        None
    };

    Ok(ImportResult {
        backup_id,
        local_path,
        remote_path,
        database: options.database,
        backup_type: options.backup_type.to_string(),
        size_bytes,
        imported_at: timestamp,
        original_source: options.source,
    })
}

async fn import_local_backup(
    source: &Path,
    backup_dir: &Path,
    options: &ImportBackupOptions,
) -> Result<(PathBuf, u64)> {
    info!("[import-backup] Importing local backup from {:?}", source);

    match options.backup_type {
        ImportBackupType::PgDump => {
            // Copy pg_dump file
            let file_name = source
                .file_name()
                .ok_or_else(|| anyhow!("Invalid source path"))?;
            let dest_path = backup_dir.join(file_name);
            std::fs::copy(source, &dest_path)?;

            let size = std::fs::metadata(&dest_path)?.len();
            info!(
                "[import-backup] Copied pg_dump file ({} bytes) to {:?}",
                size, dest_path
            );
            Ok((backup_dir.to_path_buf(), size))
        }
        ImportBackupType::PgBasebackup => {
            // Copy entire directory
            copy_directory(source, backup_dir)?;
            let size = calculate_dir_size(backup_dir)?;
            info!(
                "[import-backup] Copied pg_basebackup directory ({} bytes) to {:?}",
                size, backup_dir
            );
            Ok((backup_dir.to_path_buf(), size))
        }
        ImportBackupType::Custom => {
            // Handle custom backup format
            if source.is_dir() {
                copy_directory(source, backup_dir)?;
            } else {
                let file_name = source
                    .file_name()
                    .ok_or_else(|| anyhow!("Invalid source path"))?;
                let dest_path = backup_dir.join(file_name);
                std::fs::copy(source, &dest_path)?;
            }
            let size = calculate_dir_size(backup_dir)?;
            Ok((backup_dir.to_path_buf(), size))
        }
    }
}

async fn import_s3_backup(
    source: &str,
    backup_dir: &Path,
    options: &ImportBackupOptions,
) -> Result<(PathBuf, u64)> {
    info!("[import-backup] Importing backup from S3: {}", source);

    let storage = options
        .storage
        .as_ref()
        .ok_or_else(|| anyhow!("Storage options required for S3 import"))?;

    // Parse S3 URL: s3://bucket/key
    let url = source
        .strip_prefix("s3://")
        .ok_or_else(|| anyhow!("Invalid S3 URL"))?;
    let (bucket, key) = url
        .split_once('/')
        .ok_or_else(|| anyhow!("Invalid S3 URL format"))?;

    // Create storage provider
    let provider = storage::StorageProviderFactory::create_s3_provider(
        storage.region.clone(),
        storage.endpoint.clone(),
        storage.access_key.clone(),
        storage.secret_key.clone(),
    )
    .await?;

    // Download the backup
    let dest_file = backup_dir.join(
        Path::new(key)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("backup")),
    );
    provider.download_file(bucket, key, &dest_file).await?;

    let size = std::fs::metadata(&dest_file)?.len();
    info!(
        "[import-backup] Downloaded S3 object ({} bytes) to {:?}",
        size, dest_file
    );

    Ok((backup_dir.to_path_buf(), size))
}

#[allow(clippy::too_many_arguments)]
fn create_import_metadata(
    backup_id: &str,
    database: &str,
    backup_type: &ImportBackupType,
    size_bytes: u64,
    timestamp: DateTime<Utc>,
    original_source: &str,
    tenant: &Option<String>,
    cluster: &Option<String>,
) -> BackupMetadata {
    let storage_backup_type = match backup_type {
        ImportBackupType::PgDump => StorageBackupType::Snapshot,
        ImportBackupType::PgBasebackup => StorageBackupType::Full,
        ImportBackupType::Custom => StorageBackupType::Snapshot,
    };

    let mut tags = vec![
        format!("imported=true"),
        format!("original_source={}", original_source),
        format!("import_type={}", backup_type),
        format!("database={}", database),
    ];

    if let Some(t) = tenant {
        tags.push(format!("tenant={}", t));
    }
    if let Some(c) = cluster {
        tags.push(format!("cluster={}", c));
    }

    BackupMetadata {
        id: backup_id.to_string(),
        backup_type: storage_backup_type,
        status: StorageBackupStatus::Completed,
        start_time: timestamp,
        end_time: Some(timestamp),
        base_backup_id: None,
        wal_start: None,
        wal_end: None,
        size_bytes,
        server_version: "unknown".to_string(),
        checksum: None,
        files: vec![],
        tags,
        pinned: false,
        encrypted: None,
        encryption_algorithm: None,
    }
}

async fn upload_imported_backup(
    backup_dir: &Path,
    backup_id: &str,
    options: &ImportBackupOptions,
    storage: &StorageOptions,
) -> Result<Option<String>> {
    info!("[import-backup] Uploading imported backup to remote storage");

    let bucket = storage
        .bucket
        .as_ref()
        .ok_or_else(|| anyhow!("Storage bucket required for upload"))?;

    // Build storage key
    let prefix = storage.prefix.as_deref().unwrap_or("");
    let key_prefix = if let Some(ref tenant) = options.tenant {
        if let Some(ref cluster) = options.cluster {
            format!(
                "{}{}/{}/pg/{}/{}",
                prefix, tenant, cluster, options.database, backup_id
            )
        } else {
            format!("{}{}/pg/{}/{}", prefix, tenant, options.database, backup_id)
        }
    } else {
        format!("{}{}/{}", prefix, options.database, backup_id)
    };

    // Create storage provider
    let provider = storage::StorageProviderFactory::create_s3_provider(
        storage.region.clone(),
        storage.endpoint.clone(),
        storage.access_key.clone(),
        storage.secret_key.clone(),
    )
    .await?;

    // Upload all files in backup directory
    for entry in walkdir::WalkDir::new(backup_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let relative_path = entry.path().strip_prefix(backup_dir)?;
        let key = format!("{}/{}", key_prefix, relative_path.display());

        provider
            .upload_file(bucket, &key, entry.path(), None, None)
            .await?;
        info!("[import-backup] Uploaded: {}", key);
    }

    let remote_path = format!("s3://{}/{}", bucket, key_prefix);
    info!("[import-backup] Upload complete: {}", remote_path);

    Ok(Some(remote_path))
}

// ============================================================================
// Utility Functions
// ============================================================================

fn copy_directory(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;

    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let relative_path = entry.path().strip_prefix(src)?;
        let dest_path = dst.join(relative_path);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }

    Ok(())
}

fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;

    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        total += entry.metadata()?.len();
    }

    Ok(total)
}

// ============================================================================
// Output Formatting
// ============================================================================

/// Format discovery result for display
pub fn format_discovery_result(result: &DiscoveryResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result).unwrap_or_default(),
        "yaml" => serde_yml::to_string(result).unwrap_or_default(),
        _ => format_discovery_table(result),
    }
}

fn format_discovery_table(result: &DiscoveryResult) -> String {
    let mut output = String::new();

    // Header
    output.push_str(
        "\n╔══════════════════════════════════════════════════════════════════════════════╗\n",
    );
    output.push_str(
        "║                     PostgreSQL Discovery Report                              ║\n",
    );
    output.push_str(
        "╚══════════════════════════════════════════════════════════════════════════════╝\n\n",
    );

    // Server Info
    output.push_str(
        "┌─ Server Information ─────────────────────────────────────────────────────────┐\n",
    );
    output.push_str(&format!("│ Host:     {}:{}\n", result.host, result.port));
    output.push_str(&format!("│ Version:  {}\n", result.version));
    output.push_str(&format!("│ Role:     {}\n", result.replication.role));
    output.push_str(
        "└──────────────────────────────────────────────────────────────────────────────┘\n\n",
    );

    // Databases
    output.push_str(
        "┌─ Databases ──────────────────────────────────────────────────────────────────┐\n",
    );
    output.push_str(&format!(
        "│ {:20} {:>12} {:15} {:10}\n",
        "Name", "Size", "Owner", "Template"
    ));
    output.push_str(
        "│ ────────────────────────────────────────────────────────────────────────────\n",
    );

    for db in &result.databases {
        let size_str = format_size(db.size_bytes);
        let template_str = if db.is_template { "Yes" } else { "No" };
        output.push_str(&format!(
            "│ {:20} {:>12} {:15} {:10}\n",
            truncate_str(&db.name, 20),
            size_str,
            truncate_str(&db.owner, 15),
            template_str
        ));
    }
    output.push_str(
        "└──────────────────────────────────────────────────────────────────────────────┘\n\n",
    );

    // Replication
    output.push_str(
        "┌─ Replication Status ─────────────────────────────────────────────────────────┐\n",
    );
    output.push_str(&format!(
        "│ WAL Archiving:  {}\n",
        if result.replication.wal_archiving_enabled {
            "Enabled"
        } else {
            "Disabled"
        }
    ));
    if let Some(ref lsn) = result.replication.current_wal_lsn {
        output.push_str(&format!("│ Current LSN:    {}\n", lsn));
    }
    if !result.replication.replicas.is_empty() {
        output.push_str(&format!(
            "│ Replicas:       {} connected\n",
            result.replication.replicas.len()
        ));
        for replica in &result.replication.replicas {
            output.push_str(&format!(
                "│   - {} ({}) @ {}\n",
                replica.application_name,
                replica.state,
                replica.client_addr.as_deref().unwrap_or("unknown")
            ));
        }
    }
    if let Some(lag) = result.replication.replication_lag_bytes {
        output.push_str(&format!("│ Replication Lag: {} bytes\n", lag));
    }
    output.push_str(
        "└──────────────────────────────────────────────────────────────────────────────┘\n\n",
    );

    // Recommendations
    output.push_str(
        "┌─ Recommendations ────────────────────────────────────────────────────────────┐\n",
    );
    for (i, rec) in result.recommendations.iter().enumerate() {
        output.push_str(&format!("│ {}. {}\n", i + 1, rec));
    }
    output.push_str(
        "└──────────────────────────────────────────────────────────────────────────────┘\n",
    );

    output
}

/// Format generated config for display
pub fn format_generated_config(config: &GeneratedConfig, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(config).unwrap_or_default(),
        _ => {
            let mut output = String::new();
            output.push_str("\n=== Generated Cluster Configuration ===\n\n");
            output.push_str(&config.cluster_yaml);
            output.push_str("\n\n=== Generated Schedule Configuration ===\n\n");
            output.push_str(&config.schedule_config);
            output.push_str("\n\n=== Generated Retention Policy ===\n\n");
            output.push_str(&config.retention_policy);
            if let Some(ref path) = config.output_path {
                output.push_str(&format!("\n\nConfiguration files written to: {:?}\n", path));
            }
            output
        }
    }
}

/// Format import result for display
pub fn format_import_result(result: &ImportResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result).unwrap_or_default(),
        "yaml" => serde_yml::to_string(result).unwrap_or_default(),
        _ => {
            let mut output = String::new();
            output.push_str("\n┌─ Backup Import Result ───────────────────────────────────────────────────────┐\n");
            output.push_str(&format!("│ Backup ID:       {}\n", result.backup_id));
            output.push_str(&format!("│ Database:        {}\n", result.database));
            output.push_str(&format!("│ Type:            {}\n", result.backup_type));
            output.push_str(&format!(
                "│ Size:            {}\n",
                format_size(result.size_bytes)
            ));
            output.push_str(&format!("│ Local Path:      {:?}\n", result.local_path));
            if let Some(ref remote) = result.remote_path {
                output.push_str(&format!("│ Remote Path:     {}\n", remote));
            }
            output.push_str(&format!("│ Original Source: {}\n", result.original_source));
            output.push_str(&format!(
                "│ Imported At:     {}\n",
                result.imported_at.to_rfc3339()
            ));
            output.push_str("└──────────────────────────────────────────────────────────────────────────────┘\n");
            output.push_str("\nThe imported backup is now available for restore operations.\n");
            output.push_str(&format!(
                "Use: warden postgresql restore-full --backup-id {}\n",
                result.backup_id
            ));
            output
        }
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncate_at = s
            .char_indices()
            .take_while(|(i, _)| *i < max_len - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..truncate_at])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_backup_type_from_str() {
        assert_eq!(
            "pg_dump".parse::<ImportBackupType>().unwrap(),
            ImportBackupType::PgDump
        );
        assert_eq!(
            "pgdump".parse::<ImportBackupType>().unwrap(),
            ImportBackupType::PgDump
        );
        assert_eq!(
            "pg_basebackup".parse::<ImportBackupType>().unwrap(),
            ImportBackupType::PgBasebackup
        );
        assert_eq!(
            "custom".parse::<ImportBackupType>().unwrap(),
            ImportBackupType::Custom
        );
        assert!("invalid".parse::<ImportBackupType>().is_err());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 1024), "1.00 TB");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(truncate_str("this is a long string", 10), "this is...");
    }

    #[test]
    fn test_truncate_str_utf8_edge_cases() {
        // Test with multi-byte UTF-8 characters (emoji and special chars)
        assert_eq!(truncate_str("Hello 世界", 20), "Hello 世界");
        assert_eq!(truncate_str("Hello 世界 🦀", 8), "Hello...");
        // Ensure truncation doesn't panic on multi-byte boundaries
        // "Café ☕" with max_len=6 should safely truncate at character boundary
        let result = truncate_str("Café ☕", 6);
        assert!(result.ends_with("..."));
        // Test truncation at exact multi-byte character boundary
        let result2 = truncate_str("Test™™™™™™™™™", 10);
        assert!(result2.ends_with("..."));
        // Test that it doesn't panic - the important part
    }
}
