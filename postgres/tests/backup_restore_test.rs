use chrono::Utc;
use postgres::common::{Backup, BackupStatus, BackupType, PostgresConfig, RestoreStatus};
use postgres::manager::PostgresManager;
use tempfile::tempdir;

use tokio_postgres::{connect, NoTls};
use uuid::Uuid;

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::time::sleep;

struct TestDatabaseContext {
    runtime_config: PostgresConfig,
    maintenance_config: PostgresConfig,
}

async fn run_with_test_database<F, Fut>(test: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnOnce(TestDatabaseContext) -> Fut,
    Fut: Future<Output = Result<(), Box<dyn std::error::Error>>>,
{
    let script_path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("scripts")
        .join("setup_replication.sh");

    let base_image = GenericImage::new("postgres", "15-alpine")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections",
        ));

    let container_request = base_image
        .with_env_var("POSTGRES_DB", "postgres")
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_HOST_AUTH_METHOD", "trust")
        .with_env_var(
            "POSTGRES_INITDB_ARGS",
            "--auth-host=trust --auth-local=trust",
        )
        .with_copy_to(
            "/docker-entrypoint-initdb.d/setup_replication.sh",
            script_path,
        );

    let container = container_request.start().await?;
    let port = container.get_host_port_ipv4(5432).await?;

    let runtime_config = PostgresConfig {
        host: "127.0.0.1".to_string(),
        port,
        database: "postgres".to_string(),
        user: "postgres".to_string(),
        password: Some("postgres".to_string()),
        ssl_mode: None,
        maintenance_db: None,
        ssh_host: None,
        ssh_user: None,
        ssh_port: None,
        ssh_password: None,
        ssh_key_path: None,
        ssh_local_port: None,
        ssh_remote_port: None,
    };

    wait_for_database(&runtime_config).await?;

    let maintenance_config = make_maintenance_config(&runtime_config);

    let ctx = TestDatabaseContext {
        runtime_config,
        maintenance_config,
    };

    let result = test(ctx).await;

    drop(container);

    result
}

fn make_maintenance_config(base: &PostgresConfig) -> PostgresConfig {
    let mut cfg = base.clone();
    cfg.maintenance_db = Some("template1".to_string());
    cfg
}

async fn wait_for_database(config: &PostgresConfig) -> Result<(), Box<dyn std::error::Error>> {
    const MAX_ATTEMPTS: usize = 20;
    const DELAY: Duration = Duration::from_millis(500);

    for attempt in 1..=MAX_ATTEMPTS {
        match connect(&config.connection_string(), NoTls).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        log::error!("Test DB connection error: {}", e);
                    }
                });
                drop(client);
                return Ok(());
            }
            Err(e) => {
                if attempt == MAX_ATTEMPTS {
                    return Err(Box::new(e));
                }
                sleep(DELAY).await;
            }
        }
    }

    Err("PostgreSQL test container did not become ready".into())
}

// This test requires a running PostgreSQL instance
#[tokio::test]
#[serial_test::serial]
async fn test_full_backup_and_restore() -> Result<(), Box<dyn std::error::Error>> {
    run_with_test_database(|ctx| async move {
        let TestDatabaseContext {
            runtime_config,
            maintenance_config,
        } = ctx;

        let backup_dir = tempdir()?;
        let restore_dir = tempdir()?;

        let mut backup_manager =
            PostgresManager::new(runtime_config.clone(), backup_dir.path().to_path_buf())?;
        let backup = backup_manager.full_backup().await?;
        assert_eq!(backup.backup_type, BackupType::Full);
        drop(backup_manager);

        let mut restore_manager =
            PostgresManager::new(maintenance_config.clone(), backup_dir.path().to_path_buf())?;
        let restore = restore_manager
            .restore_full_backup(&backup.id, restore_dir.path().to_path_buf())
            .await?;
        drop(restore_manager);

        assert_eq!(restore.status, RestoreStatus::Completed);

        wait_for_database(&runtime_config).await?;

        assert!(restore_dir
            .path()
            .read_dir()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
            .next()
            .is_some());

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn test_incremental_backup_and_restore() -> Result<(), Box<dyn std::error::Error>> {
    run_with_test_database(|ctx| async move {
        let TestDatabaseContext {
            runtime_config,
            maintenance_config,
        } = ctx;

        let backup_dir = tempdir()?;
        let restore_dir = tempdir()?;

        let mut backup_manager =
            PostgresManager::new(runtime_config.clone(), backup_dir.path().to_path_buf())?;
        let full_backup = backup_manager.full_backup().await?;
        let incremental_backup = backup_manager.incremental_backup().await?;

        assert_eq!(incremental_backup.backup_type, BackupType::Incremental);
        drop(backup_manager);

        let mut restore_manager =
            PostgresManager::new(maintenance_config.clone(), backup_dir.path().to_path_buf())?;
        let restore = restore_manager
            .restore_incremental_backup(&full_backup.id, restore_dir.path().to_path_buf())
            .await?;
        drop(restore_manager);

        assert_eq!(restore.status, RestoreStatus::Completed);

        wait_for_database(&runtime_config).await?;

        assert!(restore_dir
            .path()
            .read_dir()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
            .next()
            .is_some());

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn test_point_in_time_restore() -> Result<(), Box<dyn std::error::Error>> {
    run_with_test_database(|ctx| async move {
        let TestDatabaseContext {
            runtime_config,
            maintenance_config,
        } = ctx;

        let backup_dir = tempdir()?;
        let restore_dir = tempdir()?;

        let mut backup_manager =
            PostgresManager::new(runtime_config.clone(), backup_dir.path().to_path_buf())?;

        // Create a user table and insert data before backup
        {
            let (client, connection) = connect(&runtime_config.connection_string(), NoTls).await?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    log::error!("Connection error: {}", e);
                }
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

        let full_backup = backup_manager.full_backup().await?;
        let _ = backup_manager.incremental_backup().await?;
        let target_time = Utc::now();
        drop(backup_manager);

        let mut restore_manager =
            PostgresManager::new(maintenance_config.clone(), backup_dir.path().to_path_buf())?;
        let restore = restore_manager
            .restore_point_in_time(
                &full_backup.id,
                restore_dir.path().to_path_buf(),
                target_time,
            )
            .await?;
        drop(restore_manager);

        assert_eq!(restore.status, RestoreStatus::Completed);

        wait_for_database(&runtime_config).await?;

        let (client, connection) = connect(&runtime_config.connection_string(), NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                log::error!("Connection error: {}", e);
            }
        });
        let rows = client.query("SELECT 1", &[]).await?;
        assert_eq!(rows.len(), 1);

        let tables = vec!["pg_tables", "pg_class", "pg_index"];
        for table in tables {
            let row = client
                .query_one(&format!("SELECT COUNT(*) FROM {}", table), &[])
                .await?;
            let count: i64 = row.get(0);
            assert!(count > 0, "Table {} not found", table);
        }

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

        assert!(restore_dir
            .path()
            .read_dir()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?
            .next()
            .is_some());

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn test_snapshot_backup() -> Result<(), Box<dyn std::error::Error>> {
    run_with_test_database(|ctx| async move {
        let TestDatabaseContext {
            runtime_config,
            maintenance_config: _,
        } = ctx;

        let backup_dir = tempdir()?;

        let mut manager = PostgresManager::new(runtime_config, backup_dir.path().to_path_buf())?;

        let backup = manager.snapshot_backup().await?;

        assert_eq!(backup.backup_type, BackupType::Snapshot);
        assert!(backup.backup_path.exists());

        Ok(())
    })
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn test_backup_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let backup_dir = tempdir()?;
    let catalog_path = backup_dir.path().join("backup_catalog.json");

    run_with_test_database(|ctx| async move {
        let TestDatabaseContext {
            runtime_config,
            maintenance_config: _,
        } = ctx;

        let mut manager =
            PostgresManager::new(runtime_config.clone(), backup_dir.path().to_path_buf())?;

        // Add a mock backup to the catalog
        let backup_id = Uuid::new_v4();
        let backup_path = backup_dir
            .path()
            .join(format!("snapshot_{}.dump", backup_id));

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
        let manager2 = PostgresManager::new(runtime_config, backup_dir.path().to_path_buf())?;

        // Verify that the catalog was loaded correctly
        assert_eq!(manager2.list_backups().len(), manager.list_backups().len());

        // Verify that the backup is in the catalog
        let backups = manager2.list_backups();
        assert!(backups.iter().any(|b| b.id == backup.id));

        Ok(())
    })
    .await
}
