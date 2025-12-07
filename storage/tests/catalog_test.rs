//! Integration tests for backup catalog operations.
//!
//! These tests verify listing, inspecting, and downloading backups from S3-compatible storage.
//! They require MinIO or similar S3-compatible storage to be running.

use chrono::Utc;
use storage::{
    catalog::BackupFilter,
    BackupFile, BackupMetadata, BackupStatus, BackupType, PostgresBackupStorage,
    StorageProviderType,
};
use tempfile::TempDir;

/// Get test configuration from environment variables
fn get_test_config() -> Option<TestConfig> {
    let endpoint = std::env::var("AWS_ENDPOINT").ok()?;
    let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok()?;
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok()?;
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    let bucket = std::env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "testbucket".to_string());

    Some(TestConfig {
        endpoint,
        access_key,
        secret_key,
        region,
        bucket,
    })
}

struct TestConfig {
    endpoint: String,
    access_key: String,
    secret_key: String,
    region: String,
    bucket: String,
}

/// Create a test storage provider
async fn create_test_storage(config: &TestConfig, prefix: &str) -> PostgresBackupStorage {
    PostgresBackupStorage::new(
        StorageProviderType::S3,
        config.bucket.clone(),
        Some(prefix.to_string()),
        Some(config.region.clone()),
        Some(config.endpoint.clone()),
        Some(config.access_key.clone()),
        Some(config.secret_key.clone()),
        None,
        None,
        None,
    )
    .await
    .expect("Failed to create storage provider")
}

/// Create a test backup with metadata
async fn create_test_backup(
    storage: &PostgresBackupStorage,
    backup_id: &str,
    backup_type: BackupType,
    tags: Vec<String>,
) -> BackupMetadata {
    let metadata = BackupMetadata {
        id: backup_id.to_string(),
        backup_type,
        status: BackupStatus::Completed,
        start_time: Utc::now() - chrono::Duration::hours(1),
        end_time: Some(Utc::now()),
        base_backup_id: None,
        wal_start: Some("0/1000000".to_string()),
        wal_end: Some("0/2000000".to_string()),
        size_bytes: 1024 * 1024, // 1 MB
        server_version: "15.0".to_string(),
        checksum: Some("abc123def456".to_string()),
        files: vec![
            BackupFile {
                name: "pg_dump.dump".to_string(),
                size: 512 * 1024,
                checksum: Some("file1checksum".to_string()),
            },
            BackupFile {
                name: "backup_label".to_string(),
                size: 256,
                checksum: Some("file2checksum".to_string()),
            },
        ],
        tags,
        pinned: false,
    };

    // Upload metadata
    storage
        .upload_backup_metadata(backup_id, &metadata)
        .await
        .expect("Failed to upload backup metadata");

    metadata
}

/// Clean up test backups
async fn cleanup_test_backup(storage: &PostgresBackupStorage, backup_id: &str) {
    let _ = storage.delete_backup(backup_id).await;
}

#[tokio::test]
async fn test_list_backups_empty() {
    let config = match get_test_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: AWS_ENDPOINT not set");
            return;
        }
    };

    let prefix = format!("test-catalog-empty-{}", uuid::Uuid::new_v4());
    let storage = create_test_storage(&config, &prefix).await;

    let filter = BackupFilter::new();
    let result = storage.list_backups_filtered(&filter).await;

    assert!(result.is_ok());
    let backups = result.unwrap();
    assert!(backups.is_empty());
}

#[tokio::test]
async fn test_list_backups_with_filter() {
    let config = match get_test_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: AWS_ENDPOINT not set");
            return;
        }
    };

    let prefix = format!("test-catalog-filter-{}", uuid::Uuid::new_v4());
    let storage = create_test_storage(&config, &prefix).await;

    // Create test backups
    let backup1_id = format!("backup-snapshot-{}", uuid::Uuid::new_v4());
    let backup2_id = format!("backup-full-{}", uuid::Uuid::new_v4());

    create_test_backup(
        &storage,
        &backup1_id,
        BackupType::Snapshot,
        vec!["env=prod".to_string()],
    )
    .await;

    create_test_backup(
        &storage,
        &backup2_id,
        BackupType::Full,
        vec!["env=staging".to_string()],
    )
    .await;

    // Test: List all backups
    let filter = BackupFilter::new();
    let result = storage.list_backups_filtered(&filter).await.unwrap();
    assert_eq!(result.len(), 2);

    // Test: Filter by backup type
    let filter = BackupFilter::new().with_backup_type(BackupType::Snapshot);
    let result = storage.list_backups_filtered(&filter).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].backup_type, BackupType::Snapshot);

    // Test: Filter by label
    let filter = BackupFilter::new().with_label("env", "prod");
    let result = storage.list_backups_filtered(&filter).await.unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].tags.contains(&"env=prod".to_string()));

    // Test: Filter with limit
    let filter = BackupFilter::new().with_limit(1);
    let result = storage.list_backups_filtered(&filter).await.unwrap();
    assert_eq!(result.len(), 1);

    // Cleanup
    cleanup_test_backup(&storage, &backup1_id).await;
    cleanup_test_backup(&storage, &backup2_id).await;
}

#[tokio::test]
async fn test_get_backup_details() {
    let config = match get_test_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: AWS_ENDPOINT not set");
            return;
        }
    };

    let prefix = format!("test-catalog-details-{}", uuid::Uuid::new_v4());
    let storage = create_test_storage(&config, &prefix).await;

    // Create test backup
    let backup_id = format!("backup-details-{}", uuid::Uuid::new_v4());
    let _metadata = create_test_backup(
        &storage,
        &backup_id,
        BackupType::Snapshot,
        vec!["env=test".to_string()],
    )
    .await;

    // Get details
    let result = storage.get_backup_details(&backup_id).await;
    assert!(result.is_ok());

    let details = result.unwrap();
    assert_eq!(details.metadata.id, backup_id);
    assert_eq!(details.metadata.backup_type, BackupType::Snapshot);
    assert_eq!(details.metadata.server_version, "15.0");
    assert_eq!(details.bucket, config.bucket);
    assert!(!details.objects.is_empty());

    // Cleanup
    cleanup_test_backup(&storage, &backup_id).await;
}

#[tokio::test]
async fn test_get_backup_details_not_found() {
    let config = match get_test_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: AWS_ENDPOINT not set");
            return;
        }
    };

    let prefix = format!("test-catalog-notfound-{}", uuid::Uuid::new_v4());
    let storage = create_test_storage(&config, &prefix).await;

    let result = storage.get_backup_details("nonexistent-backup-id").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_backup_exists() {
    let config = match get_test_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: AWS_ENDPOINT not set");
            return;
        }
    };

    let prefix = format!("test-catalog-exists-{}", uuid::Uuid::new_v4());
    let storage = create_test_storage(&config, &prefix).await;

    // Create test backup
    let backup_id = format!("backup-exists-{}", uuid::Uuid::new_v4());
    create_test_backup(&storage, &backup_id, BackupType::Full, vec![]).await;

    // Test exists
    let exists = storage.backup_exists(&backup_id).await.unwrap();
    assert!(exists);

    // Test not exists
    let exists = storage.backup_exists("nonexistent-backup").await.unwrap();
    assert!(!exists);

    // Cleanup
    cleanup_test_backup(&storage, &backup_id).await;
}

#[tokio::test]
async fn test_download_backup_verified() {
    let config = match get_test_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: AWS_ENDPOINT not set");
            return;
        }
    };

    let prefix = format!("test-catalog-download-{}", uuid::Uuid::new_v4());
    let storage = create_test_storage(&config, &prefix).await;

    // Create a real backup with actual files
    let backup_id = format!("backup-download-{}", uuid::Uuid::new_v4());
    
    // Create a temp directory with test files
    let temp_dir = TempDir::new().unwrap();
    let backup_path = temp_dir.path().join(&backup_id);
    std::fs::create_dir_all(&backup_path).unwrap();
    
    // Create test files
    let test_content = b"This is test backup content for verification";
    let dump_path = backup_path.join("pg_dump.dump");
    std::fs::write(&dump_path, test_content).unwrap();
    
    // Upload the backup
    storage
        .upload_backup(&backup_id, &backup_path, None)
        .await
        .expect("Failed to upload backup");

    // Create and upload metadata
    let metadata = BackupMetadata {
        id: backup_id.clone(),
        backup_type: BackupType::Snapshot,
        status: BackupStatus::Completed,
        start_time: Utc::now() - chrono::Duration::hours(1),
        end_time: Some(Utc::now()),
        base_backup_id: None,
        wal_start: None,
        wal_end: None,
        size_bytes: test_content.len() as u64,
        server_version: "15.0".to_string(),
        checksum: None,
        files: vec![BackupFile {
            name: "pg_dump.dump".to_string(),
            size: test_content.len() as u64,
            checksum: None, // No checksum for this test
        }],
        tags: vec![],
        pinned: false,
    };
    storage
        .upload_backup_metadata(&backup_id, &metadata)
        .await
        .unwrap();

    // Download to a new temp directory
    let download_dir = TempDir::new().unwrap();
    let result = storage
        .download_backup_verified(&backup_id, download_dir.path(), false)
        .await;

    assert!(result.is_ok());
    let download_result = result.unwrap();
    assert_eq!(download_result.backup_id, backup_id);
    assert!(download_result.files_downloaded > 0);
    assert!(download_result.bytes_downloaded > 0);

    // Verify the file was downloaded
    let downloaded_file = download_dir.path().join("pg_dump.dump");
    assert!(downloaded_file.exists());
    let downloaded_content = std::fs::read(&downloaded_file).unwrap();
    assert_eq!(downloaded_content, test_content);

    // Cleanup
    cleanup_test_backup(&storage, &backup_id).await;
}

// Unit tests for BackupFilter
mod filter_tests {
    use super::*;

    #[test]
    fn test_filter_matches_backup_type() {
        let metadata = BackupMetadata {
            id: "test".to_string(),
            backup_type: BackupType::Snapshot,
            status: BackupStatus::Completed,
            start_time: Utc::now(),
            end_time: None,
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 0,
            server_version: "15".to_string(),
            checksum: None,
            files: vec![],
            tags: vec![],
            pinned: false,
        };

        let filter = BackupFilter::new().with_backup_type(BackupType::Snapshot);
        assert!(filter.matches(&metadata));

        let filter = BackupFilter::new().with_backup_type(BackupType::Full);
        assert!(!filter.matches(&metadata));
    }

    #[test]
    fn test_filter_matches_status() {
        let metadata = BackupMetadata {
            id: "test".to_string(),
            backup_type: BackupType::Full,
            status: BackupStatus::Completed,
            start_time: Utc::now(),
            end_time: None,
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 0,
            server_version: "15".to_string(),
            checksum: None,
            files: vec![],
            tags: vec![],
            pinned: false,
        };

        let filter = BackupFilter::new().with_status(BackupStatus::Completed);
        assert!(filter.matches(&metadata));

        let filter = BackupFilter::new().with_status(BackupStatus::Failed);
        assert!(!filter.matches(&metadata));
    }

    #[test]
    fn test_filter_matches_labels() {
        let metadata = BackupMetadata {
            id: "test".to_string(),
            backup_type: BackupType::Full,
            status: BackupStatus::Completed,
            start_time: Utc::now(),
            end_time: None,
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 0,
            server_version: "15".to_string(),
            checksum: None,
            files: vec![],
            tags: vec!["env=prod".to_string(), "app=billing".to_string()],
            pinned: false,
        };

        // Single label match
        let filter = BackupFilter::new().with_label("env", "prod");
        assert!(filter.matches(&metadata));

        // Multiple labels match
        let filter = BackupFilter::new()
            .with_label("env", "prod")
            .with_label("app", "billing");
        assert!(filter.matches(&metadata));

        // Label not present
        let filter = BackupFilter::new().with_label("env", "staging");
        assert!(!filter.matches(&metadata));

        // One label matches, one doesn't
        let filter = BackupFilter::new()
            .with_label("env", "prod")
            .with_label("app", "other");
        assert!(!filter.matches(&metadata));
    }

    #[test]
    fn test_filter_matches_time_range() {
        let now = Utc::now();
        let metadata = BackupMetadata {
            id: "test".to_string(),
            backup_type: BackupType::Full,
            status: BackupStatus::Completed,
            start_time: now,
            end_time: None,
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 0,
            server_version: "15".to_string(),
            checksum: None,
            files: vec![],
            tags: vec![],
            pinned: false,
        };

        // After filter (backup is after the filter time)
        let filter = BackupFilter::new().after(now - chrono::Duration::hours(1));
        assert!(filter.matches(&metadata));

        // After filter (backup is before the filter time)
        let filter = BackupFilter::new().after(now + chrono::Duration::hours(1));
        assert!(!filter.matches(&metadata));

        // Before filter (backup is before the filter time)
        let filter = BackupFilter::new().before(now + chrono::Duration::hours(1));
        assert!(filter.matches(&metadata));

        // Before filter (backup is after the filter time)
        let filter = BackupFilter::new().before(now - chrono::Duration::hours(1));
        assert!(!filter.matches(&metadata));
    }

    #[test]
    fn test_filter_combined() {
        let metadata = BackupMetadata {
            id: "test".to_string(),
            backup_type: BackupType::Snapshot,
            status: BackupStatus::Completed,
            start_time: Utc::now(),
            end_time: None,
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 0,
            server_version: "15".to_string(),
            checksum: None,
            files: vec![],
            tags: vec!["env=prod".to_string()],
            pinned: false,
        };

        // All conditions match
        let filter = BackupFilter::new()
            .with_backup_type(BackupType::Snapshot)
            .with_status(BackupStatus::Completed)
            .with_label("env", "prod");
        assert!(filter.matches(&metadata));

        // One condition doesn't match
        let filter = BackupFilter::new()
            .with_backup_type(BackupType::Full) // This doesn't match
            .with_status(BackupStatus::Completed)
            .with_label("env", "prod");
        assert!(!filter.matches(&metadata));
    }
}
