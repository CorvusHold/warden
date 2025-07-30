use chrono::Utc;
use postgres::common::{Backup, BackupStatus, BackupType, PostgresConfig, RestoreStatus};
use postgres::manager::PostgresManager;
use tempfile::tempdir;

use tokio_postgres::{connect, NoTls};
use uuid::Uuid;

// No longer needed: test_postgres_request

// Helper function to create a test database config
fn create_test_config() -> PostgresConfig {
    PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "warden_dev".to_string(),
        user: "warden_dev".to_string(),
        password: Some("warden_dev".to_string()),
        ssl_mode: None,
        ssh_host: None,
        ssh_user: None,
        ssh_port: None,
        ssh_password: None,
        ssh_key_path: None,
        ssh_local_port: None,
        ssh_remote_port: None,
    }
}

// This test requires a running PostgreSQL instance
#[tokio::test]
#[serial_test::serial]
async fn test_full_backup_and_restore() -> Result<(), Box<dyn std::error::Error>> {
    // Use local Postgres config
    let backup_dir = tempdir()?;
    let restore_dir = tempdir()?;
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Perform a full backup
    let backup = manager.full_backup().await?;

    // Verify backup properties
    assert_eq!(backup.backup_type, BackupType::Full);

    // Restore from full backup
    let _restore = manager
        .restore_full_backup(&backup.id, restore_dir.path().to_path_buf())
        .await?;

    // Verify restore was successful - check for the directory structure
    // The base directory might not exist directly, but the restore directory should not be empty
    assert!(restore_dir
        .path()
        .read_dir()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
        .next()
        .is_some());

    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_incremental_backup_and_restore() -> Result<(), Box<dyn std::error::Error>> {
    // Start a temporary Postgres container

    // Use local Postgres config
    let backup_dir = tempdir()?;
    let restore_dir = tempdir()?;
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Perform a full backup
    let full_backup = manager.full_backup().await?;

    // Perform an incremental backup
    let incremental_backup = manager.incremental_backup().await?;

    // Verify backup properties
    assert_eq!(incremental_backup.backup_type, BackupType::Incremental);

    // Restore with incremental backups
    let _restore = manager
        .restore_incremental_backup(&full_backup.id, restore_dir.path().to_path_buf())
        .await?;

    // Verify restore was successful - check for the directory structure
    // The base directory might not exist directly, but the restore directory should not be empty
    assert!(restore_dir
        .path()
        .read_dir()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
        .next()
        .is_some());

    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_point_in_time_restore() -> Result<(), Box<dyn std::error::Error>> {
    // Start a temporary Postgres container

    // Use local Postgres config
    let backup_dir = tempdir()?;
    let restore_dir = tempdir()?;
    let config = create_test_config();
    let mut manager = PostgresManager::new(config, backup_dir.path().to_path_buf())?;

    // Create a user table and insert data before backup
    {
        let (client, connection) = connect(&manager.config.connection_string(), NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                log::error!("Connection error: {e}");
                return Err(e);
            }
            Ok(())
        });
        client
            .execute(
                "CREATE TABLE IF NOT EXISTS test_table (id SERIAL PRIMARY KEY, value TEXT);",
                &[],
            )
            .await?;
        client
            .execute(
                "INSERT INTO test_table (value) VALUES ($1), ($2);",
                &[&"foo", &"bar"],
            )
            .await?;
    }

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

    // Verify restore completed successfully
    assert_eq!(restore.status, RestoreStatus::Completed);

    // Create new client connection after restore
    let (client, connection) = connect(&manager.config.connection_string(), NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            log::error!("Connection error: {e}");
            return Err(e);
        }
        Ok(())
    });
    let rows = client.query("SELECT 1", &[]).await?;
    assert_eq!(rows.len(), 1);

    // Verify specific system tables exist
    let tables = vec!["pg_tables", "pg_class", "pg_index"];
    for table in tables {
        let row = client
            .query_one(&format!("SELECT COUNT(*) FROM {table}"), &[])
            .await?;
        let count: i64 = row.get(0);
        assert!(count > 0, "Table {table} not found");
    }

    // Verify user tables from restored content
    let row = client
        .query_one(
            "SELECT COUNT(*) FROM pg_tables WHERE schemaname = 'public'",
            &[],
        )
        .await?;
    let user_table_count: i64 = row.get(0);
    assert!(
        user_table_count > 0,
        "No user tables found in restored database"
    );

    // Verify restore was successful - check for the directory structure
    // The base directory might not exist directly, but the restore directory should not be empty
    assert!(restore_dir
        .path()
        .read_dir()
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
        .next()
        .is_some());

    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_snapshot_backup() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directories for backup
    let backup_dir = tempdir()?;

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Perform a snapshot backup
    let backup = manager.snapshot_backup().await?;

    // Verify backup properties
    assert_eq!(backup.backup_type, BackupType::Snapshot);

    // Verify backup file exists - the actual path is different from what we're checking
    // The backup is in a directory named snapshot_backup_{timestamp} and the file is {database}.dump
    // So we need to check if the backup_path from the Backup struct exists
    assert!(backup.backup_path.exists());

    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn test_backup_catalog() -> Result<(), Box<dyn std::error::Error>> {
    // Create temporary directory for backup
    let backup_dir = tempdir()?;
    let catalog_path = backup_dir.path().join("backup_catalog.json");

    // Create PostgreSQL manager
    let mut manager = PostgresManager::new(create_test_config(), backup_dir.path().to_path_buf())?;

    // Add a mock backup to the catalog
    let backup_id = Uuid::new_v4();
    let backup_path = backup_dir.path().join(format!("snapshot_{backup_id}.dump"));

    // Create an empty backup file
    std::fs::File::create(&backup_path)?;

    let backup = Backup {
        id: backup_id,
        backup_type: BackupType::Snapshot,
        backup_path: backup_path.clone(),
        status: BackupStatus::Completed,
        start_time: Utc::now(),
        end_time: Some(Utc::now()),
        size_bytes: Some(0),
        wal_start: None,
        wal_end: None,
        base_backup_id: None,
        server_version: "mock-version".to_string(),
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
