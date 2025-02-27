use chrono::Utc;
use postgresql::{PostgresConfig, PostgresManager};
use std::path::PathBuf;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Configure PostgreSQL connection
    let config = PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "postgres".to_string(),
        user: "postgres".to_string(),
        password: Some("postgres".to_string()),
        ssl_mode: None,
    };

    // Create backup directory
    let backup_dir = PathBuf::from("./backups");

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(config, backup_dir)?;

    // Perform a full backup
    println!("Performing full backup...");
    let full_backup = manager.full_backup().await?;
    println!("Full backup completed: {}", full_backup.id);

    // Perform an incremental backup
    println!("Performing incremental backup...");
    let incremental_backup = manager.incremental_backup().await?;
    println!("Incremental backup completed: {}", incremental_backup.id);

    // Perform a snapshot backup
    println!("Performing snapshot backup...");
    let snapshot_backup = manager.snapshot_backup().await?;
    println!("Snapshot backup completed: {}", snapshot_backup.id);

    // List all backups
    println!("\nAll backups:");
    for backup in manager.list_backups() {
        println!(
            "Backup ID: {}, Type: {:?}, Status: {:?}, Time: {}",
            backup.id, backup.backup_type, backup.status, backup.start_time
        );
    }

    // Get the latest full backup
    if let Some(latest_full) = manager.get_latest_full_backup() {
        println!("\nLatest full backup: {}", latest_full.id);

        // Restore from full backup
        let restore_dir = PathBuf::from("./restore_full");
        println!("Restoring from full backup to {:?}...", restore_dir);
        let restore = manager
            .restore_full_backup(&latest_full.id, restore_dir)
            .await?;
        println!("Restore completed: {}", restore.id);

        // Restore with incremental backups
        let restore_dir = PathBuf::from("./restore_incremental");
        println!("Restoring with incremental backups to {:?}...", restore_dir);
        let restore = manager
            .restore_incremental_backup(&latest_full.id, restore_dir)
            .await?;
        println!("Restore completed: {}", restore.id);

        // Restore to a point in time
        let restore_dir = PathBuf::from("./restore_point_in_time");
        let target_time = Utc::now(); // Use current time for example
        println!(
            "Restoring to point in time {} to {:?}...",
            target_time, restore_dir
        );
        let restore = manager
            .restore_point_in_time(&latest_full.id, restore_dir, target_time)
            .await?;
        println!("Restore completed: {}", restore.id);
    } else {
        println!("No full backups available");
    }

    Ok(())
}
