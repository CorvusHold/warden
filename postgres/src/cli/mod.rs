pub mod commands;

/// Parse a label in the format "key=value"
fn parse_label(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid label format '{}'. Expected 'key=value'",
            s
        ));
    }
    let key = parts[0].trim();
    let value = parts[1].trim();
    if key.is_empty() {
        return Err("Label key cannot be empty".to_string());
    }
    Ok((key.to_string(), value.to_string()))
}

#[derive(clap::Subcommand, Debug)]
pub enum PostgresqlCommands {
    /// Perform a full backup
    FullBackup {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Store backup in remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// Perform an incremental backup
    IncrementalBackup {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Store backup in remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// Perform a snapshot backup of PostgreSQL and optionally upload to S3-compatible storage.
    ///
    /// This command creates both physical and logical backups, stores them locally,
    /// and optionally uploads the logical backup to S3/MinIO. For remote databases,
    /// use SSH tunnel parameters to establish a secure connection.
    ///
    /// Examples:
    ///   # Local backup only
    ///   warden postgresql snapshot-backup --database mydb --user postgres
    ///
    ///   # Local backup with S3 upload
    ///   warden postgresql snapshot-backup --database mydb --user postgres \
    ///     --remote-storage --storage-bucket my-backups --storage-endpoint http://localhost:9000
    ///
    ///   # Remote database via SSH with S3 upload
    ///   warden postgresql snapshot-backup --database mydb --user postgres \
    ///     --ssh-host bastion.example.com --ssh-user ubuntu --ssh-key-path ~/.ssh/id_rsa \
    ///     --ssh-remote-port 5432 --remote-storage --storage-bucket my-backups
    #[clap(verbatim_doc_comment)]
    SnapshotBackup {
        /// PostgreSQL host (use 'localhost' when using SSH tunnel)
        #[clap(long, default_value = "localhost", env = "PGHOST")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432", env = "PGPORT")]
        port: u16,

        /// PostgreSQL database name to backup
        #[clap(long, default_value = "postgres", env = "PGDATABASE")]
        database: String,

        /// PostgreSQL user for authentication
        #[clap(long, default_value = "postgres", env = "PGUSER")]
        user: String,

        /// PostgreSQL password (prefer PGPASSWORD env var for security)
        #[clap(long, env = "PGPASSWORD")]
        password: Option<String>,

        /// PostgreSQL SSL mode (disable, allow, prefer, require, verify-ca, verify-full)
        #[clap(long, env = "PGSSLMODE")]
        ssl_mode: Option<String>,

        /// Local directory to store backup files
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Upload backup to remote S3-compatible storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (currently only 's3' is supported)
        #[clap(long, default_value = "s3")]
        storage_provider: Option<String>,

        /// S3 bucket name for storing backups
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// S3 key prefix for organizing backups (e.g., 'postgres/prod/')
        #[clap(long)]
        storage_prefix: Option<String>,

        /// AWS region for S3 (e.g., 'us-east-1', 'eu-west-1')
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Custom S3 endpoint URL (for MinIO, LocalStack, etc.)
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// AWS access key ID
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// AWS secret access key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// SSH bastion/jump host for tunneling to remote PostgreSQL
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH username for authentication
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port (default: 22)
        #[clap(long, default_value = "22")]
        ssh_port: Option<u16>,

        /// SSH password for authentication (prefer key-based auth)
        #[clap(long)]
        ssh_password: Option<String>,

        /// Path to SSH private key for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel (auto-selected if not specified)
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// Remote PostgreSQL port accessible from SSH host
        #[clap(long)]
        ssh_remote_port: Option<u16>,

        /// Labels/tags for the backup (format: key=value, can be repeated)
        /// Used for organizing backups and retention policy exceptions
        #[clap(long = "label", value_parser = parse_label)]
        labels: Vec<(String, String)>,

        // === Multi-tenant organization options ===
        /// Tenant identifier for multi-tenant storage organization
        /// When set, backups are stored under <tenant>/<cluster>/<pg>/<db>/<backup_id>/
        #[clap(long)]
        tenant: Option<String>,

        /// Cluster identifier from cluster.yaml for organizing backups by cluster
        #[clap(long)]
        cluster: Option<String>,

        /// Protection group identifier from cluster.yaml
        #[clap(long)]
        protection_group: Option<String>,
    },

    /// List all backups
    ListBackups {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// List backups from remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// Restore from a full backup
    RestoreFull {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Backup ID
        #[clap(long)]
        backup_id: String,

        /// Target directory
        #[clap(long)]
        target_dir: std::path::PathBuf,

        /// Container ID or name (for Docker or Kubernetes pod)
        #[clap(long)]
        container_id: Option<String>,

        /// Container environment type ("docker" or "kubernetes")
        #[clap(long)]
        container_type: Option<String>,

        /// Automatically restart PostgreSQL after restore
        #[clap(long)]
        auto_restart: bool,

        /// Restore from remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,

        /// Skip confirmation prompt for destructive operations
        #[clap(long)]
        yes: bool,
    },

    /// Restore with incremental backups
    RestoreIncremental {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Full backup ID
        #[clap(long)]
        full_backup_id: String,

        /// Target directory
        #[clap(long)]
        target_dir: std::path::PathBuf,

        /// Container ID or name (for Docker or Kubernetes pod)
        #[clap(long)]
        container_id: Option<String>,

        /// Container environment type ("docker" or "kubernetes")
        #[clap(long)]
        container_type: Option<String>,

        /// Automatically restart PostgreSQL after restore
        #[clap(long)]
        auto_restart: bool,

        /// Restore from remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// Restore to a point in time
    RestorePointInTime {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Full backup ID
        #[clap(long)]
        full_backup_id: String,

        /// Target directory
        #[clap(long)]
        target_dir: std::path::PathBuf,

        /// Target time (ISO 8601 format)
        #[clap(long)]
        target_time: String,

        /// Container ID or name (for Docker or Kubernetes pod)
        #[clap(long)]
        container_id: Option<String>,

        /// Container environment type ("docker" or "kubernetes")
        #[clap(long)]
        container_type: Option<String>,

        /// Automatically restart PostgreSQL after restore
        #[clap(long)]
        auto_restart: bool,

        /// Restore from remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// Restore from a snapshot backup
    RestoreSnapshot {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Backup ID
        #[clap(long)]
        backup_id: String,

        /// Target directory
        #[clap(long)]
        target_dir: std::path::PathBuf,

        /// Container ID or name (for Docker or Kubernetes pod)
        #[clap(long)]
        container_id: Option<String>,

        /// Container environment type ("docker" or "kubernetes")
        #[clap(long)]
        container_type: Option<String>,

        /// Automatically restart PostgreSQL after restore
        #[clap(long)]
        auto_restart: bool,

        /// Restore from remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// List contents of a snapshot backup
    ListSnapshotContents {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432")]
        port: u16,

        /// PostgreSQL database
        #[clap(long, default_value = "postgres")]
        database: String,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres")]
        user: String,

        /// PostgreSQL password
        #[clap(long)]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long)]
        ssl_mode: Option<String>,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Backup ID
        #[clap(long)]
        backup_id: String,

        /// List from remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long)]
        storage_provider: Option<String>,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// SSH host for port forwarding
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH user for port forwarding
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port for port forwarding
        #[clap(long)]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// SSH private key path for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// SSH remote port for port forwarding
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// Inspect detailed backup metadata from remote storage
    InspectBackup {
        /// Backup ID
        #[clap(long)]
        backup_id: String,

        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,
    },

    /// Download backup from remote storage
    DownloadBackup {
        /// Backup ID
        #[clap(long)]
        backup_id: String,

        /// Target directory for downloaded backup
        #[clap(long)]
        target_dir: std::path::PathBuf,

        /// Verify checksums after download
        #[clap(long)]
        verify_checksums: bool,

        /// Backup directory (source for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,
    },

    /// Initialize or update retention policy for a storage bucket
    InitRetentionPolicy {
        /// Path to retention policy JSON file
        #[clap(long)]
        policy_file: std::path::PathBuf,

        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,
    },

    /// Show current retention policy for a storage bucket
    ShowRetentionPolicy {
        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,
    },

    /// Evaluate purge policy (dry run - shows what would be deleted)
    PurgePlan {
        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// Output format (table, json, yaml)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Execute purge according to retention policy (DELETES backups)
    Purge {
        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// Actually execute purge (default is dry-run)
        #[clap(long)]
        apply: bool,

        /// Skip confirmation prompt
        #[clap(long)]
        yes: bool,
    },

    /// Reconstruct metadata for existing backups without metadata files
    ReconstructMetadata {
        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long)]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long)]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long)]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long)]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long)]
        storage_secret_key: Option<String>,

        /// PostgreSQL server version (if known)
        #[clap(long, default_value = "unknown")]
        server_version: String,

        /// Dry run - show what would be created without creating metadata
        #[clap(long)]
        dry_run: bool,

        /// Skip computing checksums (faster but less accurate)
        #[clap(long)]
        skip_checksums: bool,
    },

    /// Compute and display a Point-in-Time Recovery (PITR) plan.
    ///
    /// This command analyzes available backups and WAL segments to determine
    /// if recovery to the specified target time is possible. It shows the
    /// base backup that would be used and the WAL segments required.
    ///
    /// Examples:
    ///   # Plan recovery to a specific time
    ///   warden postgresql pitr-plan --target-time 2025-01-15T10:30:00Z --backup-dir ./backups
    ///
    ///   # Plan recovery using remote storage
    ///   warden postgresql pitr-plan --target-time 2025-01-15T10:30:00Z \
    ///     --remote-storage --storage-bucket my-backups
    #[clap(verbatim_doc_comment)]
    PitrPlan {
        /// Target time for recovery (RFC3339 format, e.g., 2025-01-15T10:30:00Z)
        #[clap(long)]
        target_time: String,

        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory (if separate from backup directory)
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// WAL prefix in remote storage
        #[clap(long, default_value = "wal/")]
        wal_prefix: String,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Execute Point-in-Time Recovery (PITR) to restore a database to a specific time.
    ///
    /// This command restores a PostgreSQL database to the specified target time
    /// using a base backup and WAL replay. The target directory will contain
    /// a recovered PostgreSQL data directory ready to start.
    ///
    /// Examples:
    ///   # Restore to a specific time
    ///   warden postgresql pitr-restore --target-time 2025-01-15T10:30:00Z \
    ///     --backup-dir ./backups --target-dir /var/lib/postgresql/data-recovered
    ///
    ///   # Restore from remote storage with auto-start
    ///   warden postgresql pitr-restore --target-time 2025-01-15T10:30:00Z \
    ///     --remote-storage --storage-bucket my-backups \
    ///     --target-dir /var/lib/postgresql/data-recovered --auto-start
    ///
    ///   # Interactive mode with guided prompts
    ///   warden postgresql pitr-restore --interactive
    #[clap(verbatim_doc_comment)]
    PitrRestore {
        /// Target time for recovery (RFC3339 format, e.g., 2025-01-15T10:30:00Z)
        #[clap(long, required_unless_present = "interactive")]
        target_time: Option<String>,

        /// Target directory for the recovered database
        #[clap(long, required_unless_present = "interactive")]
        target_dir: Option<std::path::PathBuf>,

        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory (if separate from backup directory)
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// WAL prefix in remote storage
        #[clap(long, default_value = "wal/")]
        wal_prefix: String,

        /// Automatically start PostgreSQL after recovery
        #[clap(long)]
        auto_start: bool,

        /// Path to PostgreSQL binaries (e.g., /usr/lib/postgresql/15/bin)
        #[clap(long)]
        pg_bin_dir: Option<std::path::PathBuf>,

        /// Skip confirmation prompt
        #[clap(long)]
        yes: bool,

        /// Run in interactive mode with guided prompts
        #[clap(long, short = 'i')]
        interactive: bool,
    },

    /// List available recovery options and time windows.
    ///
    /// Shows available base backups and WAL coverage to help determine
    /// valid recovery targets.
    #[clap(verbatim_doc_comment)]
    PitrList {
        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory (if separate from backup directory)
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// WAL prefix in remote storage
        #[clap(long, default_value = "wal/")]
        wal_prefix: String,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Full restore of a PostgreSQL backup to a replacement instance.
    ///
    /// This command restores a complete backup to a target PostgreSQL instance,
    /// supporting failover and cluster evolution scenarios. It works entirely
    /// with local configuration and S3; no HOLD or C2 connection is required.
    ///
    /// The restore process includes:
    /// - Preflight validation (backup existence, target state, required tools)
    /// - Backup download from S3 (if using remote storage)
    /// - Database preparation (terminate connections, drop/create database)
    /// - Content restoration (using pg_restore or psql)
    /// - Health verification
    ///
    /// Examples:
    ///   # Restore from local backup, replacing existing database
    ///   warden postgresql full-restore --backup-id abc123 --database mydb --user postgres
    ///
    ///   # Restore from S3 to a new database name
    ///   warden postgresql full-restore --backup-id abc123 --database mydb \
    ///     --target-database mydb_restored --remote-storage --storage-bucket backups
    ///
    ///   # Restore via SSH tunnel to remote PostgreSQL
    ///   warden postgresql full-restore --backup-id abc123 --database mydb \
    ///     --ssh-host bastion.example.com --ssh-user ubuntu --ssh-key-path ~/.ssh/id_rsa
    #[clap(verbatim_doc_comment)]
    FullRestore {
        /// Backup identifier (UUID or backup directory name)
        #[clap(long)]
        backup_id: String,

        /// Target PostgreSQL host
        #[clap(long, default_value = "localhost", env = "PGHOST")]
        host: String,

        /// Target PostgreSQL port
        #[clap(long, default_value = "5432", env = "PGPORT")]
        port: u16,

        /// Source database name (from backup)
        #[clap(long, env = "PGDATABASE")]
        database: String,

        /// Target database name (if different from source, creates new database)
        #[clap(long)]
        target_database: Option<String>,

        /// PostgreSQL user for authentication
        #[clap(long, default_value = "postgres", env = "PGUSER")]
        user: String,

        /// PostgreSQL password (prefer PGPASSWORD env var for security)
        #[clap(long, env = "PGPASSWORD")]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long, env = "PGSSLMODE")]
        ssl_mode: Option<String>,

        /// Local directory containing backups
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Skip confirmation prompts (required for destructive operations)
        #[clap(long)]
        yes: bool,

        /// Dry run - show restore plan without executing
        #[clap(long)]
        dry_run: bool,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,

        /// Download backup from remote S3-compatible storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (currently only 's3' is supported)
        #[clap(long, default_value = "s3")]
        storage_provider: Option<String>,

        /// S3 bucket name containing backups
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// S3 key prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// AWS region for S3
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Custom S3 endpoint URL (for MinIO, LocalStack, etc.)
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// AWS access key ID
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// AWS secret access key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// SSH bastion/jump host for tunneling to remote PostgreSQL
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH username for authentication
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port (default: 22)
        #[clap(long, default_value = "22")]
        ssh_port: Option<u16>,

        /// SSH password for authentication (prefer key-based auth)
        #[clap(long)]
        ssh_password: Option<String>,

        /// Path to SSH private key for authentication
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel (auto-selected if not specified)
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// Remote PostgreSQL port accessible from SSH host
        #[clap(long)]
        ssh_remote_port: Option<u16>,
    },

    /// Compute and display a retention plan (dry-run).
    ///
    /// This command evaluates which backups and WAL segments would be kept or
    /// deleted according to the retention policy, without making any changes.
    /// Use this to preview the effects of a retention policy before applying it.
    ///
    /// Examples:
    ///   # Plan using a local policy file
    ///   warden postgresql retention-plan --policy-file ./retention-policy.json --backup-dir ./backups
    ///
    ///   # Plan using remote storage policy
    ///   warden postgresql retention-plan --remote-storage --storage-bucket my-backups
    ///
    ///   # Plan for both local and remote backups
    ///   warden postgresql retention-plan --policy-file ./policy.json \
    ///     --backup-dir ./backups --remote-storage --storage-bucket my-backups
    #[clap(verbatim_doc_comment)]
    RetentionPlan {
        /// Path to retention policy file (JSON or YAML)
        #[clap(long)]
        policy_file: Option<std::path::PathBuf>,

        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory (if separate from backup directory)
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Include local backups in evaluation
        #[clap(long, default_value = "true")]
        include_local: bool,

        /// Include remote backups in evaluation
        #[clap(long)]
        include_remote: bool,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json, yaml)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Apply retention policy and delete expired backups.
    ///
    /// This command executes the retention policy, deleting backups and WAL
    /// segments that are outside the retention window. By default, it runs
    /// in dry-run mode; use --apply to actually delete backups.
    ///
    /// CAUTION: This operation is destructive and cannot be undone.
    ///
    /// Examples:
    ///   # Dry-run (show what would be deleted)
    ///   warden postgresql retention-apply --policy-file ./policy.json --backup-dir ./backups
    ///
    ///   # Actually delete backups (with confirmation)
    ///   warden postgresql retention-apply --policy-file ./policy.json --backup-dir ./backups --apply
    ///
    ///   # Delete without confirmation (for automation)
    ///   warden postgresql retention-apply --policy-file ./policy.json --backup-dir ./backups --apply --yes
    #[clap(verbatim_doc_comment)]
    RetentionApply {
        /// Path to retention policy file (JSON or YAML)
        #[clap(long)]
        policy_file: Option<std::path::PathBuf>,

        /// Backup directory (for local backups)
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory (if separate from backup directory)
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Include local backups in evaluation
        #[clap(long, default_value = "true")]
        include_local: bool,

        /// Include remote backups in evaluation
        #[clap(long)]
        include_remote: bool,

        /// Use remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage provider type (s3)
        #[clap(long, default_value = "s3")]
        storage_provider: String,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Actually execute deletions (default is dry-run)
        #[clap(long)]
        apply: bool,

        /// Skip confirmation prompt
        #[clap(long)]
        yes: bool,

        /// Output format (table, json, yaml)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Generate a sample retention policy file.
    ///
    /// Creates a retention policy file with sensible defaults that can be
    /// customized for your needs. Supports different presets for common
    /// use cases.
    ///
    /// Examples:
    ///   # Generate standard policy
    ///   warden postgresql retention-init --output ./retention-policy.json
    ///
    ///   # Generate conservative policy (longer retention)
    ///   warden postgresql retention-init --preset conservative --output ./policy.json
    ///
    ///   # Generate aggressive policy (shorter retention)
    ///   warden postgresql retention-init --preset aggressive --output ./policy.json
    #[clap(verbatim_doc_comment)]
    RetentionInit {
        /// Output file path for the policy
        #[clap(long, short = 'o')]
        output: std::path::PathBuf,

        /// Policy preset (standard, conservative, aggressive, gfs)
        #[clap(long, default_value = "standard")]
        preset: String,

        /// Output format (json, yaml)
        #[clap(long, default_value = "json")]
        format: String,
    },

    /// Manage backup catalog - list, inspect, and download backups from S3-compatible storage.
    ///
    /// These commands provide offline-first backup management, working entirely
    /// with local CLI and S3-compatible storage without requiring HOLD or C2.
    #[clap(subcommand)]
    Backups(BackupsCommands),

    /// Validate a cluster configuration file.
    ///
    /// Checks the cluster configuration for syntax errors, duplicate IDs,
    /// invalid references, and other semantic issues. Returns a detailed
    /// report of any problems found.
    ///
    /// Examples:
    ///   # Validate default cluster config
    ///   warden postgresql cluster-validate
    ///
    ///   # Validate specific config file
    ///   warden postgresql cluster-validate --config ./cluster.yaml
    ///
    ///   # Interactive mode with fix suggestions
    ///   warden postgresql cluster-validate --interactive
    #[clap(verbatim_doc_comment)]
    ClusterValidate {
        /// Path to cluster configuration file (YAML)
        /// If not specified, searches default paths: ./cluster.yaml, ~/.warden/cluster.yaml, /etc/warden/cluster.yaml
        #[clap(long, short = 'c')]
        config: Option<std::path::PathBuf>,

        /// Run in interactive mode with fix suggestions
        #[clap(long, short = 'i')]
        interactive: bool,
    },

    /// Display cluster configuration overview.
    ///
    /// Shows a summary of all clusters defined in the configuration,
    /// including their environments, node counts, and protection groups.
    ///
    /// Examples:
    ///   # Show cluster overview in table format
    ///   warden postgresql cluster-show
    ///
    ///   # Show cluster overview as JSON
    ///   warden postgresql cluster-show --format json
    #[clap(verbatim_doc_comment)]
    ClusterShow {
        /// Path to cluster configuration file (YAML)
        #[clap(long, short = 'c')]
        config: Option<std::path::PathBuf>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// List all nodes in the cluster configuration.
    ///
    /// Displays detailed information about each node including its role,
    /// host, port, and cluster membership.
    ///
    /// Examples:
    ///   # List all nodes
    ///   warden postgresql cluster-nodes
    ///
    ///   # List nodes for a specific cluster
    ///   warden postgresql cluster-nodes --cluster prod-billing
    ///
    ///   # List only primary nodes
    ///   warden postgresql cluster-nodes --role primary
    ///
    ///   # Output as JSON
    ///   warden postgresql cluster-nodes --format json
    #[clap(verbatim_doc_comment)]
    ClusterNodes {
        /// Path to cluster configuration file (YAML)
        #[clap(long, short = 'c')]
        config: Option<std::path::PathBuf>,

        /// Filter by cluster ID
        #[clap(long)]
        cluster: Option<String>,

        /// Filter by node role (primary, replica, unknown)
        #[clap(long)]
        role: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// List all protection groups in the cluster configuration.
    ///
    /// Shows protection groups with their associated databases and
    /// preferred backup source roles.
    ///
    /// Examples:
    ///   # List all protection groups
    ///   warden postgresql cluster-protection-groups
    ///
    ///   # List protection groups for a specific cluster
    ///   warden postgresql cluster-protection-groups --cluster prod-billing
    ///
    ///   # Output as JSON
    ///   warden postgresql cluster-protection-groups --format json
    #[clap(verbatim_doc_comment)]
    ClusterProtectionGroups {
        /// Path to cluster configuration file (YAML)
        #[clap(long, short = 'c')]
        config: Option<std::path::PathBuf>,

        /// Filter by cluster ID
        #[clap(long)]
        cluster: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// List all configured backup and retention schedules.
    ///
    /// Shows schedules defined in the Warden configuration file, including
    /// their cron expressions, targets, and enabled status.
    ///
    /// Examples:
    ///   # List all schedules
    ///   warden postgresql schedule-list
    ///
    ///   # List schedules as JSON
    ///   warden postgresql schedule-list --format json
    ///
    ///   # List only enabled schedules
    ///   warden postgresql schedule-list --enabled-only
    #[clap(verbatim_doc_comment)]
    ScheduleList {
        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,

        /// Show only enabled schedules
        #[clap(long)]
        enabled_only: bool,

        /// Filter by schedule type (backup, retention)
        #[clap(long = "type")]
        schedule_type: Option<String>,
    },

    /// Show the next scheduled runs for backup and retention tasks.
    ///
    /// Displays when each schedule will next execute, sorted by time.
    /// Useful for verifying schedule configuration and debugging.
    ///
    /// Examples:
    ///   # Show next runs for all schedules
    ///   warden postgresql schedule-next-runs
    ///
    ///   # Show next 10 runs
    ///   warden postgresql schedule-next-runs --count 10
    ///
    ///   # Show next runs as JSON
    ///   warden postgresql schedule-next-runs --format json
    #[clap(verbatim_doc_comment)]
    ScheduleNextRuns {
        /// Number of upcoming runs to show per schedule
        #[clap(long, default_value = "5")]
        count: usize,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,

        /// Show only enabled schedules
        #[clap(long)]
        enabled_only: bool,
    },

    /// Validate schedule configuration.
    ///
    /// Checks that all cron expressions are valid and that referenced
    /// storage profiles exist.
    ///
    /// Examples:
    ///   # Validate schedules
    ///   warden postgresql schedule-validate
    #[clap(verbatim_doc_comment)]
    ScheduleValidate,

    /// Run a specific schedule immediately (for testing).
    ///
    /// Executes the specified schedule now, regardless of its cron expression.
    /// Use --dry-run to see what would happen without executing.
    ///
    /// Examples:
    ///   # Dry-run a schedule
    ///   warden postgresql schedule-run --id daily-backup --dry-run
    ///
    ///   # Actually run a schedule
    ///   warden postgresql schedule-run --id daily-backup
    #[clap(verbatim_doc_comment)]
    ScheduleRun {
        /// Schedule ID to run
        #[clap(long)]
        id: String,

        /// Dry-run mode (show what would happen without executing)
        #[clap(long)]
        dry_run: bool,
    },

    /// Show overall data protection status for PostgreSQL.
    ///
    /// Displays a high-level summary of backup health, PITR coverage,
    /// retention policy status, and storage usage. This is the primary
    /// command for operators to assess data protection posture.
    ///
    /// Examples:
    ///   # Show status for local backups
    ///   warden postgresql status --backup-dir ./backups
    ///
    ///   # Show status including remote storage
    ///   warden postgresql status --backup-dir ./backups --storage-bucket my-backups
    ///
    ///   # Output as JSON for automation
    ///   warden postgresql status --backup-dir ./backups --format json
    #[clap(verbatim_doc_comment)]
    Status {
        /// Local backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory (for PITR analysis)
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Retention policy file path
        #[clap(long)]
        retention_policy: Option<std::path::PathBuf>,

        /// Database name for context
        #[clap(long)]
        database: Option<String>,

        /// Host for context
        #[clap(long)]
        host: Option<String>,

        /// Include remote storage in status
        #[clap(long)]
        remote_storage: bool,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL (for MinIO, LocalStack, etc.)
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,

        /// Maximum backup age before warning (hours)
        #[clap(long, default_value = "24")]
        backup_warning_age_hours: u32,

        /// Maximum backup age before critical (hours)
        #[clap(long, default_value = "48")]
        backup_critical_age_hours: u32,

        // === Multi-tenant organization options ===
        /// Tenant identifier for filtering backups
        #[clap(long)]
        tenant: Option<String>,

        /// Cluster identifier for filtering backups
        #[clap(long)]
        cluster: Option<String>,

        /// Protection group identifier for filtering backups
        #[clap(long)]
        protection_group: Option<String>,

        /// Include legacy (non-tenant) backups in listing
        #[clap(long)]
        include_legacy: bool,
    },

    /// Show detailed backup status.
    ///
    /// Displays information about backup history, last successful backup,
    /// failed backups, and backup frequency.
    ///
    /// Examples:
    ///   # Show backup status
    ///   warden postgresql backup-status --backup-dir ./backups
    ///
    ///   # Show backup status for a specific database
    ///   warden postgresql backup-status --backup-dir ./backups --database mydb
    #[clap(verbatim_doc_comment)]
    BackupStatus {
        /// Local backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Database name to filter by
        #[clap(long)]
        database: Option<String>,

        /// Include remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Show PITR (Point-in-Time Recovery) status.
    ///
    /// Displays the current PITR window, available recovery points,
    /// WAL segment coverage, and any gaps in coverage.
    ///
    /// Examples:
    ///   # Show PITR status
    ///   warden postgresql pitr-status --backup-dir ./backups
    ///
    ///   # Show PITR status with WAL archive
    ///   warden postgresql pitr-status --backup-dir ./backups --wal-archive-dir ./wal_archive
    #[clap(verbatim_doc_comment)]
    PitrStatus {
        /// Local backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Database name for context
        #[clap(long)]
        database: Option<String>,

        /// Include remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Export metrics in Prometheus format.
    ///
    /// Outputs metrics suitable for scraping by Prometheus or writing
    /// to a text file for node_exporter textfile collector.
    ///
    /// Examples:
    ///   # Output metrics to stdout
    ///   warden postgresql metrics --backup-dir ./backups
    ///
    ///   # Write metrics to a file
    ///   warden postgresql metrics --backup-dir ./backups --output /var/lib/node_exporter/warden.prom
    ///
    ///   # Output as JSON
    ///   warden postgresql metrics --backup-dir ./backups --format json
    #[clap(verbatim_doc_comment)]
    Metrics {
        /// Local backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// WAL archive directory
        #[clap(long)]
        wal_archive_dir: Option<std::path::PathBuf>,

        /// Database name label
        #[clap(long)]
        database: Option<String>,

        /// Host label
        #[clap(long)]
        host: Option<String>,

        /// Include remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output file path (stdout if not specified)
        #[clap(long, short = 'o')]
        output: Option<std::path::PathBuf>,

        /// Output format (prometheus, json)
        #[clap(long, default_value = "prometheus")]
        format: String,
    },

    /// Discover an existing PostgreSQL instance for migration to Warden.
    ///
    /// Connects to a PostgreSQL server and gathers information about:
    /// - PostgreSQL version
    /// - Database list with sizes
    /// - Replication status (primary/replica)
    /// - Current backup configuration (if detectable)
    /// - Recommended Warden configuration
    ///
    /// Examples:
    ///   # Discover local PostgreSQL
    ///   warden postgresql discover --host localhost --port 5432 --user postgres
    ///
    ///   # Discover remote PostgreSQL via SSH
    ///   warden postgresql discover --host db.example.com --port 5432 --user postgres \
    ///     --ssh-host bastion.example.com --ssh-user ubuntu --ssh-key-path ~/.ssh/id_rsa
    ///
    ///   # Output as JSON for scripting
    ///   warden postgresql discover --host localhost --user postgres --format json
    #[clap(verbatim_doc_comment)]
    Discover {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost", env = "PGHOST")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432", env = "PGPORT")]
        port: u16,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres", env = "PGUSER")]
        user: String,

        /// PostgreSQL password (prefer PGPASSWORD env var for security)
        #[clap(long, env = "PGPASSWORD")]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long, env = "PGSSLMODE")]
        ssl_mode: Option<String>,

        /// SSH bastion/jump host for tunneling
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH username
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port (default: 22)
        #[clap(long, default_value = "22")]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// Path to SSH private key
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// Remote PostgreSQL port accessible from SSH host
        #[clap(long)]
        ssh_remote_port: Option<u16>,

        /// Output format (table, json, yaml)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Generate Warden configuration from an existing PostgreSQL instance.
    ///
    /// Connects to a PostgreSQL server, discovers its configuration, and generates:
    /// - cluster.yaml with discovered node(s)
    /// - Suggested schedule configuration
    /// - Suggested retention policy
    ///
    /// Examples:
    ///   # Generate config and print to stdout
    ///   warden postgresql generate-config --host localhost --user postgres
    ///
    ///   # Generate config and write to directory
    ///   warden postgresql generate-config --host localhost --user postgres \
    ///     --output ./warden-config
    ///
    ///   # Generate config with custom cluster name and tenant
    ///   warden postgresql generate-config --host localhost --user postgres \
    ///     --cluster-name "prod-billing" --tenant "acme-corp" --output ./config
    ///
    ///   # Interactive mode with guided prompts
    ///   warden postgresql generate-config --interactive
    #[clap(verbatim_doc_comment)]
    GenerateConfig {
        /// PostgreSQL host
        #[clap(long, default_value = "localhost", env = "PGHOST")]
        host: String,

        /// PostgreSQL port
        #[clap(long, default_value = "5432", env = "PGPORT")]
        port: u16,

        /// PostgreSQL user
        #[clap(long, default_value = "postgres", env = "PGUSER")]
        user: String,

        /// PostgreSQL password (prefer PGPASSWORD env var for security)
        #[clap(long, env = "PGPASSWORD")]
        password: Option<String>,

        /// PostgreSQL SSL mode
        #[clap(long, env = "PGSSLMODE")]
        ssl_mode: Option<String>,

        /// Cluster name for the generated configuration
        #[clap(long)]
        cluster_name: Option<String>,

        /// Tenant identifier for multi-tenant organization
        #[clap(long)]
        tenant: Option<String>,

        /// Output directory for generated configuration files
        #[clap(long, short = 'o')]
        output: Option<std::path::PathBuf>,

        /// Run in interactive mode with guided prompts
        #[clap(long, short = 'i')]
        interactive: bool,

        /// SSH bastion/jump host for tunneling
        #[clap(long)]
        ssh_host: Option<String>,

        /// SSH username
        #[clap(long)]
        ssh_user: Option<String>,

        /// SSH port (default: 22)
        #[clap(long, default_value = "22")]
        ssh_port: Option<u16>,

        /// SSH password for authentication
        #[clap(long)]
        ssh_password: Option<String>,

        /// Path to SSH private key
        #[clap(long)]
        ssh_key_path: Option<String>,

        /// Local port for SSH tunnel
        #[clap(long)]
        ssh_local_port: Option<u16>,

        /// Remote PostgreSQL port accessible from SSH host
        #[clap(long)]
        ssh_remote_port: Option<u16>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Import an existing backup into Warden's catalog.
    ///
    /// Supports importing from:
    /// - Local pg_dump files (.sql, .dump, .tar)
    /// - Local pg_basebackup directories
    /// - Existing S3 paths (copies to Warden's layout)
    ///
    /// The imported backup will have proper metadata generated so it can be
    /// used for restore and PITR operations.
    ///
    /// Examples:
    ///   # Import a pg_dump file
    ///   warden postgresql import-backup --source ./backup.dump \
    ///     --backup-type pg_dump --database mydb
    ///
    ///   # Import a pg_basebackup directory
    ///   warden postgresql import-backup --source ./pg_basebackup_dir \
    ///     --backup-type pg_basebackup --database postgres
    ///
    ///   # Import from S3 and organize by tenant/cluster
    ///   warden postgresql import-backup --source s3://old-bucket/backup.dump \
    ///     --backup-type pg_dump --database mydb \
    ///     --tenant acme-corp --cluster prod-billing \
    ///     --storage-bucket warden-backups
    ///
    ///   # Import and upload to Warden's S3 storage
    ///   warden postgresql import-backup --source ./backup.dump \
    ///     --backup-type pg_dump --database mydb \
    ///     --remote-storage --storage-bucket warden-backups
    #[clap(verbatim_doc_comment)]
    ImportBackup {
        /// Source path or S3 URL (s3://bucket/key)
        #[clap(long)]
        source: String,

        /// Type of backup: pg_dump, pg_basebackup, or custom
        #[clap(long)]
        backup_type: String,

        /// Database name the backup is for
        #[clap(long)]
        database: String,

        /// Tenant identifier for multi-tenant organization
        #[clap(long)]
        tenant: Option<String>,

        /// Cluster identifier for organizing backups
        #[clap(long)]
        cluster: Option<String>,

        /// Storage profile name (from config)
        #[clap(long)]
        storage_profile: Option<String>,

        /// Local backup directory for storing imported backup
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Upload imported backup to remote storage
        #[clap(long)]
        remote_storage: bool,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL (for MinIO, LocalStack, etc.)
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },
}

/// Subcommands for backup catalog management
#[derive(clap::Subcommand, Debug)]
pub enum BackupsCommands {
    /// List backups from remote storage with optional filtering.
    ///
    /// Examples:
    ///   # List all backups
    ///   warden postgresql backups list --storage-bucket my-backups
    ///
    ///   # List only snapshot backups
    ///   warden postgresql backups list --storage-bucket my-backups --type snapshot
    ///
    ///   # List backups from the last 7 days
    ///   warden postgresql backups list --storage-bucket my-backups --after 2025-01-08
    ///
    ///   # List backups with specific labels
    ///   warden postgresql backups list --storage-bucket my-backups --label env=prod
    #[clap(verbatim_doc_comment)]
    List {
        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL (for MinIO, LocalStack, etc.)
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Filter by backup type (full, incremental, snapshot)
        #[clap(long = "type")]
        backup_type: Option<String>,

        /// Filter by database name
        #[clap(long)]
        database: Option<String>,

        /// Filter by status (completed, in_progress, failed)
        #[clap(long)]
        status: Option<String>,

        /// Filter backups after this timestamp (RFC3339 or YYYY-MM-DD)
        #[clap(long)]
        after: Option<String>,

        /// Filter backups before this timestamp (RFC3339 or YYYY-MM-DD)
        #[clap(long)]
        before: Option<String>,

        /// Filter by label (format: key=value, can be repeated)
        #[clap(long = "label", value_parser = parse_label)]
        labels: Vec<(String, String)>,

        /// Maximum number of backups to return
        #[clap(long)]
        limit: Option<usize>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Show detailed information about a specific backup.
    ///
    /// Examples:
    ///   # Show backup details in table format
    ///   warden postgresql backups show --backup-id abc123 --storage-bucket my-backups
    ///
    ///   # Show backup details as JSON
    ///   warden postgresql backups show --backup-id abc123 --storage-bucket my-backups --format json
    #[clap(verbatim_doc_comment)]
    Show {
        /// Backup ID to inspect
        #[clap(long)]
        backup_id: String,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL (for MinIO, LocalStack, etc.)
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Download a backup from remote storage to local disk.
    ///
    /// Examples:
    ///   # Download backup to a directory
    ///   warden postgresql backups download --backup-id abc123 --output ./restored-backup \
    ///     --storage-bucket my-backups
    ///
    ///   # Download and verify checksums
    ///   warden postgresql backups download --backup-id abc123 --output ./restored-backup \
    ///     --storage-bucket my-backups --verify-checksums
    #[clap(verbatim_doc_comment)]
    Download {
        /// Backup ID to download
        #[clap(long)]
        backup_id: String,

        /// Target directory for downloaded backup
        #[clap(long, short = 'o')]
        output: std::path::PathBuf,

        /// Verify checksums after download
        #[clap(long)]
        verify_checksums: bool,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage prefix for backups
        #[clap(long)]
        storage_prefix: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage endpoint URL (for MinIO, LocalStack, etc.)
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Perform a planned switchover from primary to a prepared replica.
    ///
    /// This command gracefully transfers the primary role from one node to
    /// a designated replica. It includes pre-flight checks, replication
    /// catchup verification, and post-switchover validation.
    ///
    /// Examples:
    ///   # Dry-run to see the plan
    ///   warden postgresql ha-switchover --cluster prod-billing \
    ///     --from-node billing-primary --to-node billing-replica-1 --dry-run
    ///
    ///   # Execute switchover with confirmation
    ///   warden postgresql ha-switchover --cluster prod-billing \
    ///     --from-node billing-primary --to-node billing-replica-1
    ///
    ///   # Execute without confirmation prompt
    ///   warden postgresql ha-switchover --cluster prod-billing \
    ///     --from-node billing-primary --to-node billing-replica-1 --yes
    ///
    ///   # Interactive mode with guided prompts
    ///   warden postgresql ha-switchover --interactive
    #[clap(verbatim_doc_comment)]
    HaSwitchover {
        /// Cluster ID from cluster.yaml
        #[clap(long, required_unless_present = "interactive")]
        cluster: Option<String>,

        /// Source node ID (current primary)
        #[clap(long, required_unless_present = "interactive")]
        from_node: Option<String>,

        /// Target node ID (replica to promote)
        #[clap(long, required_unless_present = "interactive")]
        to_node: Option<String>,

        /// Path to cluster configuration file
        #[clap(long, short = 'c')]
        config: Option<std::path::PathBuf>,

        /// Dry-run mode (show plan without executing)
        #[clap(long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[clap(long)]
        yes: bool,

        /// Maximum replication lag in bytes before switchover (default: 1MB)
        #[clap(long, default_value = "1048576")]
        max_lag_bytes: u64,

        /// Timeout for replication catchup in seconds
        #[clap(long, default_value = "60")]
        catchup_timeout: u64,

        /// PostgreSQL user for connections
        #[clap(long, default_value = "postgres", env = "PGUSER")]
        pg_user: String,

        /// PostgreSQL password
        #[clap(long, env = "PGPASSWORD")]
        pg_password: Option<String>,

        /// Database name for connections
        #[clap(long, default_value = "postgres", env = "PGDATABASE")]
        database: String,

        /// Data directory of the target node (for promotion)
        #[clap(long)]
        target_data_dir: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,

        /// Run in interactive mode with guided prompts
        #[clap(long, short = 'i')]
        interactive: bool,
    },

    /// Perform an emergency failover to promote a replica when primary is down.
    ///
    /// This command promotes a replica to primary when the current primary is
    /// unavailable. It includes verification that the primary is unreachable
    /// (unless --force is used) and optional PITR to a specific point in time.
    ///
    /// ⚠️  WARNING: This is a destructive operation. Any transactions not yet
    /// replicated to the target node will be lost.
    ///
    /// Examples:
    ///   # Dry-run to see the plan
    ///   warden postgresql ha-failover --cluster prod-billing \
    ///     --to-node billing-replica-1 --dry-run
    ///
    ///   # Execute failover (verifies primary is down)
    ///   warden postgresql ha-failover --cluster prod-billing \
    ///     --to-node billing-replica-1 --yes
    ///
    ///   # Force failover without checking primary
    ///   warden postgresql ha-failover --cluster prod-billing \
    ///     --to-node billing-replica-1 --force --yes
    ///
    ///   # Failover with PITR to specific time
    ///   warden postgresql ha-failover --cluster prod-billing \
    ///     --to-node billing-replica-1 --target-time 2025-01-15T10:30:00Z --yes
    #[clap(verbatim_doc_comment)]
    HaFailover {
        /// Cluster ID from cluster.yaml
        #[clap(long)]
        cluster: String,

        /// Target node ID (replica to promote)
        #[clap(long)]
        to_node: String,

        /// Optional target time for PITR-based failover (RFC3339 format)
        #[clap(long)]
        target_time: Option<String>,

        /// Path to cluster configuration file
        #[clap(long, short = 'c')]
        config: Option<std::path::PathBuf>,

        /// Dry-run mode (show plan without executing)
        #[clap(long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[clap(long)]
        yes: bool,

        /// Force failover even if primary is reachable
        #[clap(long)]
        force: bool,

        /// PostgreSQL user for connections
        #[clap(long, default_value = "postgres", env = "PGUSER")]
        pg_user: String,

        /// PostgreSQL password
        #[clap(long, env = "PGPASSWORD")]
        pg_password: Option<String>,

        /// Database name for connections
        #[clap(long, default_value = "postgres", env = "PGDATABASE")]
        database: String,

        /// Data directory of the target node (for promotion)
        #[clap(long)]
        target_data_dir: Option<String>,

        /// Backup directory for PITR
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },

    /// Create a new replica from an existing backup or PITR point.
    ///
    /// This command creates a new PostgreSQL replica by restoring from a backup
    /// and configuring it to stream from the primary. It can optionally perform
    /// PITR to a specific point in time.
    ///
    /// Examples:
    ///   # Dry-run to see the plan
    ///   warden postgresql ha-clone-node --cluster prod-billing \
    ///     --source-node billing-primary --target-node billing-replica-2 \
    ///     --target-dir /var/lib/postgresql/data --dry-run
    ///
    ///   # Clone from latest backup
    ///   warden postgresql ha-clone-node --cluster prod-billing \
    ///     --source-node billing-primary --target-node billing-replica-2 \
    ///     --target-dir /var/lib/postgresql/data --yes
    ///
    ///   # Clone from specific backup
    ///   warden postgresql ha-clone-node --cluster prod-billing \
    ///     --source-node billing-primary --target-node billing-replica-2 \
    ///     --backup-id abc123 --target-dir /var/lib/postgresql/data --yes
    ///
    ///   # Clone with PITR to specific time
    ///   warden postgresql ha-clone-node --cluster prod-billing \
    ///     --source-node billing-primary --target-node billing-replica-2 \
    ///     --target-time 2025-01-15T10:30:00Z --target-dir /var/lib/postgresql/data --yes
    #[clap(verbatim_doc_comment)]
    HaCloneNode {
        /// Cluster ID from cluster.yaml
        #[clap(long)]
        cluster: String,

        /// Source node ID (to get backup from)
        #[clap(long)]
        source_node: String,

        /// Target node ID (new replica)
        #[clap(long)]
        target_node: String,

        /// Specific backup ID to use
        #[clap(long)]
        backup_id: Option<String>,

        /// Optional target time for PITR-based clone (RFC3339 format)
        #[clap(long)]
        target_time: Option<String>,

        /// Target directory for the new replica data
        #[clap(long)]
        target_dir: std::path::PathBuf,

        /// Path to cluster configuration file
        #[clap(long, short = 'c')]
        config: Option<std::path::PathBuf>,

        /// Dry-run mode (show plan without executing)
        #[clap(long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[clap(long)]
        yes: bool,

        /// Backup directory
        #[clap(long, default_value = "./backups")]
        backup_dir: std::path::PathBuf,

        /// PostgreSQL user for connections
        #[clap(long, default_value = "postgres", env = "PGUSER")]
        pg_user: String,

        /// PostgreSQL password
        #[clap(long, env = "PGPASSWORD")]
        pg_password: Option<String>,

        /// Database name for connections
        #[clap(long, default_value = "postgres", env = "PGDATABASE")]
        database: String,

        /// Use remote storage for backups
        #[clap(long)]
        remote_storage: bool,

        /// Storage bucket name
        #[clap(long, env = "AWS_BUCKET")]
        storage_bucket: Option<String>,

        /// Storage endpoint URL
        #[clap(long, env = "AWS_ENDPOINT")]
        storage_endpoint: Option<String>,

        /// Storage region
        #[clap(long, env = "AWS_REGION")]
        storage_region: Option<String>,

        /// Storage access key
        #[clap(long, env = "AWS_ACCESS_KEY_ID")]
        storage_access_key: Option<String>,

        /// Storage secret key
        #[clap(long, env = "AWS_SECRET_ACCESS_KEY")]
        storage_secret_key: Option<String>,

        /// Output format (table, json)
        #[clap(long, default_value = "table")]
        format: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_label_valid() {
        let result = parse_label("env=prod");
        assert!(result.is_ok());
        let (key, value) = result.unwrap();
        assert_eq!(key, "env");
        assert_eq!(value, "prod");
    }

    #[test]
    fn test_parse_label_with_spaces() {
        let result = parse_label(" key = value ");
        assert!(result.is_ok());
        let (key, value) = result.unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "value");
    }

    #[test]
    fn test_parse_label_value_with_equals() {
        // Value can contain equals sign
        let result = parse_label("config=key=value");
        assert!(result.is_ok());
        let (key, value) = result.unwrap();
        assert_eq!(key, "config");
        assert_eq!(value, "key=value");
    }

    #[test]
    fn test_parse_label_empty_value() {
        let result = parse_label("key=");
        assert!(result.is_ok());
        let (key, value) = result.unwrap();
        assert_eq!(key, "key");
        assert_eq!(value, "");
    }

    #[test]
    fn test_parse_label_missing_equals() {
        let result = parse_label("invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Expected 'key=value'"));
    }

    #[test]
    fn test_parse_label_empty_key() {
        let result = parse_label("=value");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("key cannot be empty"));
    }
}
