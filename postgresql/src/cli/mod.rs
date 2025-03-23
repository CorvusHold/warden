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
    },
}
