pub mod commands;

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

    /// Perform a snapshot backup
    SnapshotBackup {
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
}
