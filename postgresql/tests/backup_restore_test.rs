use chrono::Utc;
use postgresql::common::{Backup, BackupStatus, BackupType, PostgresConfig};
use postgresql::manager::PostgresManager;
use tempfile::tempdir;
use uuid::Uuid;

// Helper function to create a test database config
fn create_test_config() -> PostgresConfig {
    PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "postgres".to_string(),
        user: "postgres".to_string(),
        password: Some("postgres".to_string()),
        ssl_mode: None,
    }
}

// This test requires a running PostgreSQL instance
#[tokio::test]
#[ignore] // Ignore by default as it requires a running PostgreSQL instance
async fn test_full_backup_and_restore() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directories for backup and restore
    let backup_dir = tempdir()?;
    let restore_dir = tempdir()?;

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Perform a full backup
    let backup = manager.full_backup().await?;

    // Verify backup properties
    assert_eq!(backup.backup_type, BackupType::Full);

    // Restore from full backup
    let restore = manager
        .restore_full_backup(&backup.id, restore_dir.path().to_path_buf())
        .await?;

    // Verify restore was successful
    assert!(restore_dir.path().join("base").exists());

    Ok(())
}

#[tokio::test]
#[ignore] // Ignore by default as it requires a running PostgreSQL instance
async fn test_incremental_backup_and_restore() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directories for backup and restore
    let backup_dir = tempdir()?;
    let restore_dir = tempdir()?;

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Perform a full backup
    let full_backup = manager.full_backup().await?;

    // Perform an incremental backup
    let incremental_backup = manager.incremental_backup().await?;

    // Verify backup properties
    assert_eq!(incremental_backup.backup_type, BackupType::Incremental);

    // Restore with incremental backups
    let restore = manager
        .restore_incremental_backup(&full_backup.id, restore_dir.path().to_path_buf())
        .await?;

    // Verify restore was successful
    assert!(restore_dir.path().join("base").exists());

    Ok(())
}

#[tokio::test]
#[ignore] // Ignore by default as it requires a running PostgreSQL instance
async fn test_point_in_time_restore() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directories for backup and restore
    let backup_dir = tempdir()?;
    let restore_dir = tempdir()?;

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Perform a full backup
    let full_backup = manager.full_backup().await?;

    // Perform an incremental backup
    let _ = manager.incremental_backup().await?;

    // Set target time to now
    let target_time = Utc::now();

    // Restore to point in time
    let restore = manager
        .restore_point_in_time(
            &full_backup.id,
            restore_dir.path().to_path_buf(),
            target_time,
        )
        .await?;

    // Verify restore was successful
    assert!(restore_dir.path().join("base").exists());

    Ok(())
}

#[tokio::test]
#[ignore] // Ignore by default as it requires a running PostgreSQL instance
async fn test_snapshot_backup() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directories for backup
    let backup_dir = tempdir()?;

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Perform a snapshot backup
    let backup = manager.snapshot_backup().await?;

    // Verify backup properties
    assert_eq!(backup.backup_type, BackupType::Snapshot);

    // Verify backup file exists
    let backup_path = backup_dir
        .path()
        .join(format!("snapshot_{}.sql", backup.id));
    assert!(backup_path.exists());

    Ok(())
}

#[tokio::test]
async fn test_backup_catalog() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directory for backup
    let backup_dir = tempdir()?;
    let catalog_path = backup_dir.path().join("backup_catalog.json");

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Add a mock backup to the catalog
    let backup_id = Uuid::new_v4();
    let backup_path = backup_dir
        .path()
        .join(format!("snapshot_{}.sql", backup_id));

    // Create an empty backup file
    std::fs::write(&backup_path, "-- Mock backup file")?;

    // Add the backup to the catalog
    let backup = Backup {
        id: backup_id,
        backup_type: BackupType::Snapshot,
        status: BackupStatus::Completed,
        start_time: Utc::now(),
        end_time: Some(Utc::now()),
        base_backup_id: None,
        wal_start: None,
        wal_end: None,
        size_bytes: Some(0),
        backup_path: backup_path,
        server_version: "14.0".to_string(),
        error_message: None,
    };

    let _ = manager.add_backup_to_catalog(backup.clone());

    // Verify catalog file exists
    assert!(catalog_path.exists());

    // Create a new manager with the same backup directory
    let manager2 = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Verify that the catalog was loaded correctly
    assert_eq!(manager2.list_backups().len(), manager.list_backups().len());

    // Verify that the backup is in the catalog
    let backups = manager2.list_backups();
    assert!(backups.iter().any(|b| b.id == backup.id));

    Ok(())
}
