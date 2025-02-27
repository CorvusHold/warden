use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use log::info;
use postgresql::{PostgresConfig, PostgresManager};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Parser)]
#[clap(
    name = "pg_warden",
    about = "PostgreSQL backup and restore tool",
    version = "0.1.0",
    author = "Warden Team"
)]
struct Cli {
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
    backup_dir: PathBuf,

    /// Subcommand
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform a full backup
    FullBackup,

    /// Perform an incremental backup
    IncrementalBackup,

    /// Perform a snapshot backup
    SnapshotBackup,

    /// List all backups
    ListBackups,

    /// Restore from a full backup
    RestoreFull {
        /// Backup ID
        #[clap(long)]
        backup_id: Uuid,

        /// Target directory
        #[clap(long)]
        target_dir: PathBuf,
    },

    /// Restore with incremental backups
    RestoreIncremental {
        /// Full backup ID
        #[clap(long)]
        full_backup_id: Uuid,

        /// Target directory
        #[clap(long)]
        target_dir: PathBuf,
    },

    /// Restore to a point in time
    RestorePointInTime {
        /// Full backup ID
        #[clap(long)]
        full_backup_id: Uuid,

        /// Target directory
        #[clap(long)]
        target_dir: PathBuf,

        /// Target time (ISO 8601 format)
        #[clap(long)]
        target_time: String,
    },

    /// Restore from a snapshot backup
    RestoreSnapshot {
        /// Backup ID
        #[clap(long)]
        backup_id: Uuid,

        /// Target directory
        #[clap(long)]
        target_dir: PathBuf,
    },

    /// List contents of a snapshot backup
    ListSnapshotContents {
        /// Backup ID
        #[clap(long)]
        backup_id: Uuid,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Parse command line arguments
    let cli = Cli::parse();

    // Configure PostgreSQL connection
    let config = PostgresConfig {
        host: cli.host,
        port: cli.port,
        database: cli.database,
        user: cli.user,
        password: cli.password,
        ssl_mode: cli.ssl_mode,
    };

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(config, cli.backup_dir)?;

    // Execute command
    match cli.command {
        Commands::FullBackup => {
            println!("Performing full backup...");
            let backup = manager.full_backup().await?;
            println!("Full backup completed: {}", backup.id);
        }
        Commands::IncrementalBackup => {
            println!("Performing incremental backup...");
            let backup = manager.incremental_backup().await?;
            println!("Incremental backup completed: {}", backup.id);
        }
        Commands::SnapshotBackup => {
            println!("Performing snapshot backup...");
            let backup = manager.snapshot_backup().await?;
            println!("Snapshot backup completed: {}", backup.id);
        }
        Commands::ListBackups => {
            println!("All backups:");
            for backup in manager.list_backups() {
                println!(
                    "Backup ID: {}, Type: {:?}, Status: {:?}, Time: {}",
                    backup.id, backup.backup_type, backup.status, backup.start_time
                );
            }
        }
        Commands::RestoreFull {
            backup_id,
            target_dir,
        } => {
            println!(
                "Restoring from full backup {} to {:?}...",
                backup_id, target_dir
            );
            let restore = manager.restore_full_backup(&backup_id, target_dir).await?;
            println!("Restore completed: {}", restore.id);
        }
        Commands::RestoreIncremental {
            full_backup_id,
            target_dir,
        } => {
            println!(
                "Restoring with incremental backups from {} to {:?}...",
                full_backup_id, target_dir
            );
            let restore = manager
                .restore_incremental_backup(&full_backup_id, target_dir)
                .await?;
            println!("Restore completed: {}", restore.id);
        }
        Commands::RestorePointInTime {
            full_backup_id,
            target_dir,
            target_time,
        } => {
            // Parse target time
            let target_time = DateTime::from_str(&target_time)
                .map_err(|e| format!("Invalid target time format: {}", e))?;

            println!(
                "Restoring to point in time {} from {} to {:?}...",
                target_time, full_backup_id, target_dir
            );
            let restore = manager
                .restore_point_in_time(&full_backup_id, target_dir, target_time)
                .await?;
            println!("Restore completed: {}", restore.id);
        }
        Commands::RestoreSnapshot {
            backup_id,
            target_dir,
        } => {
            println!(
                "Restoring from snapshot backup {} to {:?}...",
                backup_id, target_dir
            );
            let restore = manager
                .restore_snapshot_backup(&backup_id, target_dir)
                .await?;
            println!("Restore completed: {}", restore.id);
        }
        Commands::ListSnapshotContents { backup_id } => {
            println!("Listing contents of snapshot backup {}...", backup_id);
            let contents = manager.list_snapshot_contents(&backup_id).await?;
            println!("Contents:");
            println!("{}", contents);
        }
    }

    Ok(())
}
