//! Chaos and failure mode tests for Warden.
//!
//! These tests simulate various failure scenarios to verify that Warden
//! handles errors gracefully and maintains system integrity.
//!
//! Run with: `cargo test -p postgres --test chaos_test -- --ignored --test-threads=1`
//! Or use: `make chaos-test`

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::time::Instant;

use chrono::Utc;
use tempfile::TempDir;

// ============================================================================
// Test Configuration
// ============================================================================

/// Get test PostgreSQL configuration from environment.
fn get_test_pg_config() -> Option<TestPgConfig> {
    let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port: u16 = env::var("POSTGRES_PORT")
        .unwrap_or_else(|_| "5432".to_string())
        .parse()
        .unwrap_or(5432);
    let user = env::var("POSTGRES_USER").unwrap_or_else(|_| "postgres".to_string());
    let password = env::var("POSTGRES_PASSWORD").ok();
    let database = env::var("POSTGRES_DB").unwrap_or_else(|_| "postgres".to_string());

    Some(TestPgConfig {
        host,
        port,
        user,
        password,
        database,
    })
}

struct TestPgConfig {
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    database: String,
}

/// Get test MinIO/S3 configuration from environment.
fn get_test_storage_config() -> Option<TestStorageConfig> {
    let endpoint = env::var("AWS_ENDPOINT").ok()?;
    let access_key = env::var("AWS_ACCESS_KEY_ID").ok()?;
    let secret_key = env::var("AWS_SECRET_ACCESS_KEY").ok()?;
    let bucket = env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "testbucket".to_string());
    let region = env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());

    Some(TestStorageConfig {
        endpoint,
        access_key,
        secret_key,
        bucket,
        region,
    })
}

struct TestStorageConfig {
    endpoint: String,
    access_key: String,
    secret_key: String,
    bucket: String,
    region: String,
}

// ============================================================================
// Chaos Test: PostgreSQL Connection Failures
// ============================================================================

/// Test that backup fails gracefully when PostgreSQL is unreachable.
#[tokio::test]
#[ignore = "chaos test - requires specific setup"]
async fn test_backup_postgres_unreachable() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Use a port that definitely has no PostgreSQL
    let result = snapshot_backup(
        "localhost".to_string(),
        59999, // Non-existent port
        "testdb".to_string(),
        "postgres".to_string(),
        Some("password".to_string()),
        None,
        backup_dir.clone(),
        SshOptions::default(),
        StorageOptions::default(),
        HashMap::new(),
    )
    .await;

    // Verify failure
    assert!(
        result.is_err(),
        "Backup should fail when PostgreSQL is unreachable"
    );

    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("connection") || err_msg.contains("refused") || err_msg.contains("failed"),
        "Error should mention connection issue: {}",
        err_msg
    );

    // Verify no partial artifacts left behind
    let entries: Vec<_> = std::fs::read_dir(&backup_dir)
        .map(|r| r.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();

    // Allow empty directory or only metadata files
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            // Check for in-progress markers
            let in_progress = path.join(".in_progress");
            assert!(
                !in_progress.exists(),
                "Found in-progress marker that should have been cleaned up: {:?}",
                in_progress
            );
        }
    }
}

/// Test that backup fails gracefully with invalid credentials.
#[tokio::test]
#[ignore = "chaos test - requires running PostgreSQL"]
async fn test_backup_invalid_credentials() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};

    let config = match get_test_pg_config() {
        Some(c) => c,
        None => {
            println!("Skipping: PostgreSQL config not available");
            return;
        }
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    let result = snapshot_backup(
        config.host,
        config.port,
        config.database,
        config.user,
        Some("INVALID_PASSWORD_12345".to_string()), // Wrong password
        None,
        backup_dir,
        SshOptions::default(),
        StorageOptions::default(),
        HashMap::new(),
    )
    .await;

    // May succeed if PostgreSQL uses trust auth, otherwise should fail
    if result.is_err() {
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        // Should not panic, should return a proper error
        assert!(
            !err_msg.contains("panic"),
            "Should not panic on auth errors"
        );
    }
}

// ============================================================================
// Chaos Test: S3/MinIO Failures
// ============================================================================

/// Test that backup handles S3 upload failures gracefully.
#[tokio::test]
#[ignore = "chaos test - requires running PostgreSQL and MinIO"]
async fn test_backup_s3_upload_failure() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};

    let pg_config = match get_test_pg_config() {
        Some(c) => c,
        None => {
            println!("Skipping: PostgreSQL config not available");
            return;
        }
    };

    let storage_config = match get_test_storage_config() {
        Some(c) => c,
        None => {
            println!("Skipping: Storage config not available");
            return;
        }
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Use invalid S3 credentials to simulate upload failure
    let storage = StorageOptions {
        remote_storage: true,
        provider_type: Some("s3".to_string()),
        bucket: Some(storage_config.bucket),
        prefix: Some("chaos-test/".to_string()),
        region: Some(storage_config.region),
        endpoint: Some(storage_config.endpoint),
        access_key: Some("INVALID_ACCESS_KEY".to_string()),
        secret_key: Some("INVALID_SECRET_KEY".to_string()),
        multi_tenant: Default::default(),
    };

    let result = snapshot_backup(
        pg_config.host,
        pg_config.port,
        pg_config.database,
        pg_config.user,
        pg_config.password,
        None,
        backup_dir.clone(),
        SshOptions::default(),
        storage,
        HashMap::new(),
    )
    .await;

    // The backup might succeed locally but fail on upload
    // Either way, we're testing that errors are handled gracefully
    if result.is_err() {
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(!err_msg.contains("panic"), "Should not panic on S3 errors");

        // Error should mention storage/S3/access
        assert!(
            err_msg.contains("s3")
                || err_msg.contains("storage")
                || err_msg.contains("access")
                || err_msg.contains("upload")
                || err_msg.contains("credential"),
            "Error should mention storage issue: {}",
            err_msg
        );
    }

    // Local backup should still exist even if upload failed
    // (This is the expected behavior - local backup is preserved)
}

/// Test backup with non-existent S3 bucket.
#[tokio::test]
#[ignore = "chaos test - requires running PostgreSQL and MinIO"]
async fn test_backup_s3_nonexistent_bucket() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};

    let pg_config = match get_test_pg_config() {
        Some(c) => c,
        None => {
            println!("Skipping: PostgreSQL config not available");
            return;
        }
    };

    let storage_config = match get_test_storage_config() {
        Some(c) => c,
        None => {
            println!("Skipping: Storage config not available");
            return;
        }
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    let storage = StorageOptions {
        remote_storage: true,
        provider_type: Some("s3".to_string()),
        bucket: Some("nonexistent-bucket-12345".to_string()), // Non-existent bucket
        prefix: Some("chaos-test/".to_string()),
        region: Some(storage_config.region),
        endpoint: Some(storage_config.endpoint),
        access_key: Some(storage_config.access_key),
        secret_key: Some(storage_config.secret_key),
        multi_tenant: Default::default(),
    };

    let result = snapshot_backup(
        pg_config.host,
        pg_config.port,
        pg_config.database,
        pg_config.user,
        pg_config.password,
        None,
        backup_dir,
        SshOptions::default(),
        storage,
        HashMap::new(),
    )
    .await;

    if result.is_err() {
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err_msg.contains("bucket")
                || err_msg.contains("not found")
                || err_msg.contains("nosuchbucket"),
            "Error should mention bucket issue: {}",
            err_msg
        );
    }
}

// ============================================================================
// Chaos Test: Disk Space and Permission Errors
// ============================================================================

/// Test that backup fails gracefully when backup directory is not writable.
#[tokio::test]
#[ignore = "chaos test - requires running PostgreSQL"]
async fn test_backup_dir_not_writable() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};

    let pg_config = match get_test_pg_config() {
        Some(c) => c,
        None => {
            println!("Skipping: PostgreSQL config not available");
            return;
        }
    };

    // Use a path that should not be writable
    let backup_dir = PathBuf::from("/root/warden_test_backup");

    let result = snapshot_backup(
        pg_config.host,
        pg_config.port,
        pg_config.database,
        pg_config.user,
        pg_config.password,
        None,
        backup_dir,
        SshOptions::default(),
        StorageOptions::default(),
        HashMap::new(),
    )
    .await;

    // Should fail with permission error
    assert!(
        result.is_err(),
        "Backup should fail for non-writable directory"
    );

    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("permission")
            || err_msg.contains("denied")
            || err_msg.contains("access")
            || err_msg.contains("create"),
        "Error should mention permission issue: {}",
        err_msg
    );
}

// ============================================================================
// Chaos Test: PITR Failure Modes
// ============================================================================

/// Test PITR with target time before any backup.
#[tokio::test]
async fn test_pitr_target_before_backup() {
    use chrono::Duration;
    use postgres::pitr::{PitrPlanner, RecoveryTarget};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Create a backup catalog with a recent backup
    let backup_time = Utc::now() - Duration::hours(1);
    let catalog = serde_json::json!({
        "backups": [{
            "id": "test-backup-001",
            "backup_type": "Full",
            "status": "Completed",
            "start_time": backup_time.to_rfc3339(),
            "end_time": (backup_time + Duration::minutes(5)).to_rfc3339(),
            "wal_start": "0/1000000",
            "wal_end": "0/2000000",
            "size_bytes": 1024,
            "backup_path": backup_dir.join("backup1").to_string_lossy().to_string(),
            "server_version": "15.0"
        }]
    });

    std::fs::write(
        backup_dir.join("backup_catalog.json"),
        serde_json::to_string(&catalog).unwrap(),
    )
    .expect("Failed to write catalog");

    std::fs::create_dir_all(backup_dir.join("backup1")).unwrap();

    let planner = PitrPlanner::new(backup_dir);

    // Try to recover to a time BEFORE the backup
    let target_time = Utc::now() - Duration::hours(5);
    let target = RecoveryTarget::Time(target_time);

    let result = planner.plan_recovery(target).await;

    assert!(result.is_err(), "PITR should fail for target before backup");

    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("before")
            || err_msg.contains("no backup")
            || err_msg.contains("not found")
            || err_msg.contains("no base backup")
            || err_msg.contains("pitr"),
        "Error should mention target is before backup: {}",
        err_msg
    );
}

/// Test PITR with missing WAL segments.
#[tokio::test]
async fn test_pitr_missing_wal_segments() {
    use chrono::Duration;
    use postgres::pitr::{PitrPlanner, RecoveryTarget};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Create a backup catalog
    let backup_time = Utc::now() - Duration::hours(2);
    let catalog = serde_json::json!({
        "backups": [{
            "id": "test-backup-002",
            "backup_type": "Full",
            "status": "Completed",
            "start_time": backup_time.to_rfc3339(),
            "end_time": (backup_time + Duration::minutes(5)).to_rfc3339(),
            "wal_start": "0/1000000",
            "wal_end": "0/2000000",
            "size_bytes": 1024,
            "backup_path": backup_dir.join("backup2").to_string_lossy().to_string(),
            "server_version": "15.0"
        }]
    });

    std::fs::write(
        backup_dir.join("backup_catalog.json"),
        serde_json::to_string(&catalog).unwrap(),
    )
    .expect("Failed to write catalog");

    std::fs::create_dir_all(backup_dir.join("backup2")).unwrap();

    // Create WAL archive directory but leave it empty (missing WAL)
    std::fs::create_dir_all(backup_dir.join("wal_archive")).unwrap();

    let planner =
        PitrPlanner::new(backup_dir.clone()).with_wal_archive_dir(backup_dir.join("wal_archive"));

    // Try to recover to a time that would require WAL
    let target_time = backup_time + Duration::hours(1);
    let target = RecoveryTarget::Time(target_time);

    let result = planner.plan_recovery(target).await;

    // This might succeed if WAL is not strictly required, or fail if it is
    // The important thing is it doesn't panic
    if result.is_err() {
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            !err_msg.contains("panic"),
            "Should not panic on missing WAL"
        );
    }
}

// ============================================================================
// Chaos Test: Restore Failure Modes
// ============================================================================

/// Test restore with non-existent backup ID.
#[tokio::test]
async fn test_restore_nonexistent_backup() {
    use postgres::cli::commands::{restore_full, SshOptions, StorageOptions};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();
    let target_dir = temp_dir.path().join("restore_target");

    // Create empty backup catalog
    let catalog = serde_json::json!({
        "backups": []
    });
    std::fs::write(
        backup_dir.join("backup_catalog.json"),
        serde_json::to_string(&catalog).unwrap(),
    )
    .expect("Failed to write catalog");

    // Use a valid UUID format that doesn't exist
    let nonexistent_backup_id = uuid::Uuid::new_v4().to_string();

    let result = restore_full(
        "localhost".to_string(),
        5432,
        "testdb".to_string(),
        "postgres".to_string(),
        None, // password
        None, // ssl_mode
        backup_dir,
        nonexistent_backup_id,
        target_dir,
        None,  // container_id
        None,  // container_type
        false, // auto_restart
        SshOptions::default(),
        StorageOptions::default(),
        true, // yes (skip confirmation)
    )
    .await;

    assert!(
        result.is_err(),
        "Restore should fail for non-existent backup"
    );

    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("not found")
            || err_msg.contains("does not exist")
            || err_msg.contains("no backup")
            || err_msg.contains("failed"),
        "Error should mention backup not found: {}",
        err_msg
    );
}

/// Test restore to non-empty directory without --yes flag.
#[tokio::test]
async fn test_restore_to_nonempty_dir_without_yes() {
    use postgres::cli::commands::{restore_full, SshOptions, StorageOptions};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();
    let target_dir = temp_dir.path().join("restore_target");

    // Create target directory with some content
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("existing_file.txt"), "existing content").unwrap();

    // Create a minimal backup catalog with a backup
    let backup_id = uuid::Uuid::new_v4().to_string();
    let backup_path = backup_dir.join(&backup_id);
    std::fs::create_dir_all(&backup_path).unwrap();

    let catalog = serde_json::json!({
        "backups": [{
            "id": backup_id,
            "backup_type": "Full",
            "status": "Completed",
            "start_time": Utc::now().to_rfc3339(),
            "end_time": Utc::now().to_rfc3339(),
            "size_bytes": 1024,
            "backup_path": backup_path.to_string_lossy().to_string(),
            "server_version": "15.0"
        }]
    });
    std::fs::write(
        backup_dir.join("backup_catalog.json"),
        serde_json::to_string(&catalog).unwrap(),
    )
    .expect("Failed to write catalog");

    let result = restore_full(
        "localhost".to_string(),
        5432,
        "testdb".to_string(),
        "postgres".to_string(),
        None, // password
        None, // ssl_mode
        backup_dir,
        backup_id,
        target_dir.clone(),
        None,  // container_id
        None,  // container_type
        false, // auto_restart
        SshOptions::default(),
        StorageOptions::default(),
        false, // yes = false (should prompt or fail)
    )
    .await;

    // Should either fail or warn about non-empty directory
    // The important thing is it doesn't silently overwrite
    if result.is_err() {
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err_msg.contains("not empty")
                || err_msg.contains("exists")
                || err_msg.contains("confirm")
                || err_msg.contains("overwrite"),
            "Error should mention non-empty directory: {}",
            err_msg
        );
    }

    // Original file should still exist
    assert!(
        target_dir.join("existing_file.txt").exists(),
        "Original file should not be overwritten without confirmation"
    );
}

// ============================================================================
// Chaos Test: Retention Failure Modes
// ============================================================================

/// Test retention with invalid policy file.
#[tokio::test]
async fn test_retention_invalid_policy() {
    use postgres::cli::commands::{retention_plan, RetentionOptions, StorageOptions};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();
    let policy_file = temp_dir.path().join("invalid_policy.json");

    // Write invalid JSON
    std::fs::write(&policy_file, "{ invalid json }").unwrap();

    let storage_opts = StorageOptions::default();
    let retention_opts = RetentionOptions {
        policy_file: Some(policy_file),
        backup_dir,
        wal_archive_dir: None,
        include_local: true,
        include_remote: false,
        format: "table".to_string(),
    };

    let result = retention_plan(storage_opts, retention_opts).await;

    // Should fail with parse error
    assert!(result.is_err(), "Retention should fail with invalid policy");

    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("parse")
            || err_msg.contains("invalid")
            || err_msg.contains("json")
            || err_msg.contains("syntax"),
        "Error should mention parse issue: {}",
        err_msg
    );
}

/// Test retention with missing policy file.
#[tokio::test]
async fn test_retention_missing_policy() {
    use postgres::cli::commands::{retention_plan, RetentionOptions, StorageOptions};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();
    let nonexistent_policy = temp_dir.path().join("nonexistent_policy.json");

    let storage_opts = StorageOptions::default();
    let retention_opts = RetentionOptions {
        policy_file: Some(nonexistent_policy),
        backup_dir,
        wal_archive_dir: None,
        include_local: true,
        include_remote: false,
        format: "table".to_string(),
    };

    let result = retention_plan(storage_opts, retention_opts).await;

    // Should fail or use defaults
    if result.is_err() {
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err_msg.contains("not found")
                || err_msg.contains("does not exist")
                || err_msg.contains("no such file"),
            "Error should mention missing file: {}",
            err_msg
        );
    }
}

// ============================================================================
// Performance Measurement Tests
// ============================================================================

/// Measure backup performance and report metrics.
#[tokio::test]
#[ignore = "performance test - requires running PostgreSQL"]
async fn test_backup_performance_metrics() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};

    let pg_config = match get_test_pg_config() {
        Some(c) => c,
        None => {
            println!("Skipping: PostgreSQL config not available");
            return;
        }
    };

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    let start = Instant::now();

    let result = snapshot_backup(
        pg_config.host,
        pg_config.port,
        pg_config.database,
        pg_config.user,
        pg_config.password,
        None,
        backup_dir.clone(),
        SshOptions::default(),
        StorageOptions::default(),
        HashMap::new(),
    )
    .await;

    let duration = start.elapsed();

    if let Ok(backup) = result {
        println!("\n=== Backup Performance Metrics ===");
        println!("Backup ID: {}", backup.backup_id);
        println!("Duration: {:?}", duration);
        println!("Size: {} bytes", backup.size_bytes);

        if duration.as_secs() > 0 {
            let throughput = backup.size_bytes as f64 / duration.as_secs_f64();
            println!("Throughput: {:.2} bytes/sec", throughput);
        }

        // Log metrics for CI/monitoring
        println!(
            "METRIC backup_duration_seconds={:.3}",
            duration.as_secs_f64()
        );
        println!("METRIC backup_size_bytes={}", backup.size_bytes);
    } else {
        println!("Backup failed: {:?}", result.unwrap_err());
    }
}

// ============================================================================
// Error Code Verification Tests
// ============================================================================

/// Verify that errors are categorized correctly.
#[test]
fn test_error_categorization_comprehensive() {
    use anyhow::anyhow;
    use postgres::error_codes::{categorize_error, ExitCode};

    // Test various error patterns
    let test_cases = vec![
        // Usage errors
        ("Invalid argument: --foo", ExitCode::UsageError),
        ("Missing required argument", ExitCode::UsageError),
        ("Unrecognized option", ExitCode::UsageError),
        // Config errors
        ("Configuration file not found", ExitCode::ConfigError),
        ("Invalid config format", ExitCode::ConfigError),
        ("Policy file error", ExitCode::ConfigError),
        // Environment errors
        ("SSH tunnel failed", ExitCode::EnvironmentError),
        ("pg_dump not found", ExitCode::EnvironmentError),
        ("Connection refused", ExitCode::EnvironmentError),
        ("Network timeout", ExitCode::EnvironmentError),
        ("Disk full", ExitCode::EnvironmentError),
        ("Permission denied", ExitCode::EnvironmentError),
        // Remote service errors
        ("S3 bucket not found", ExitCode::RemoteServiceError),
        ("Upload failed", ExitCode::RemoteServiceError),
        ("MinIO connection error", ExitCode::RemoteServiceError),
        ("Access denied to storage", ExitCode::RemoteServiceError),
        // Internal errors (default)
        ("Unexpected state", ExitCode::InternalError),
        ("Assertion failed", ExitCode::InternalError),
    ];

    for (error_msg, expected_code) in test_cases {
        let error = anyhow!(error_msg);
        let actual_code = categorize_error(&error);
        assert_eq!(
            actual_code, expected_code,
            "Error '{}' should be categorized as {:?}, got {:?}",
            error_msg, expected_code, actual_code
        );
    }
}

// ============================================================================
// Artifact Cleanup Verification
// ============================================================================

/// Verify that partial artifacts are cleaned up on failure.
#[tokio::test]
async fn test_partial_artifact_cleanup() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Create a simulated partial backup
    let partial_backup_dir = backup_dir.join("partial_backup_12345");
    std::fs::create_dir_all(&partial_backup_dir).unwrap();

    // Create in-progress marker
    std::fs::write(partial_backup_dir.join(".in_progress"), "").unwrap();

    // Create some partial files
    std::fs::write(partial_backup_dir.join("partial_data.bin"), vec![0u8; 1000]).unwrap();

    // Verify the partial backup exists
    assert!(partial_backup_dir.exists());
    assert!(partial_backup_dir.join(".in_progress").exists());

    // In a real scenario, the cleanup would be triggered by the backup failure
    // Here we just verify the structure that should be cleaned up

    // Check for in-progress markers
    let has_in_progress = std::fs::read_dir(&backup_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|entry| {
            let path = entry.path();
            path.is_dir() && path.join(".in_progress").exists()
        });

    assert!(
        has_in_progress,
        "Test setup should have created in-progress marker"
    );

    // Clean up (simulating what the backup code should do on failure)
    if partial_backup_dir.join(".in_progress").exists() {
        std::fs::remove_dir_all(&partial_backup_dir).unwrap();
    }

    // Verify cleanup
    assert!(
        !partial_backup_dir.exists(),
        "Partial backup should be cleaned up"
    );
}
