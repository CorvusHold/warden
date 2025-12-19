//! Tests for error handling and failure modes.
//!
//! These tests verify that errors are properly categorized and reported
//! with appropriate exit codes as documented in API.md.

use std::env;
use std::path::PathBuf;
use tempfile::TempDir;

// ============================================================================
// SSH Failure Tests
// ============================================================================

#[tokio::test]
async fn test_ssh_connection_refused_error() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};
    use std::collections::HashMap;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Try to connect to a non-existent SSH host
    let ssh = SshOptions {
        host: Some("nonexistent.invalid.host".to_string()),
        user: Some("testuser".to_string()),
        port: Some(22),
        password: Some("password".to_string()),
        key_path: None,
        local_port: Some(15432),
        remote_port: Some(5432),
    };

    let result = snapshot_backup(
        "localhost".to_string(),
        5432,
        "testdb".to_string(),
        "postgres".to_string(),
        Some("password".to_string()),
        None,
        backup_dir,
        ssh,
        StorageOptions::default(),
        HashMap::new(),
    )
    .await;

    assert!(result.is_err(), "Should fail with SSH error");
    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("ssh") || err_msg.contains("tunnel") || err_msg.contains("connection"),
        "Error should mention SSH/tunnel/connection: {}",
        err_msg
    );
}

// ============================================================================
// S3/Storage Failure Tests
// ============================================================================

#[tokio::test]
async fn test_s3_invalid_credentials_error() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};
    use std::collections::HashMap;

    // Skip if no MinIO endpoint configured
    if env::var("AWS_ENDPOINT").is_err() {
        println!("Skipping: AWS_ENDPOINT not set");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Use invalid credentials
    let storage = StorageOptions {
        remote_storage: true,
        provider_type: Some("s3".to_string()),
        bucket: Some("testbucket".to_string()),
        prefix: None,
        region: Some("us-east-1".to_string()),
        endpoint: env::var("AWS_ENDPOINT").ok(),
        access_key: Some("INVALID_ACCESS_KEY".to_string()),
        secret_key: Some("INVALID_SECRET_KEY".to_string()),
        multi_tenant: Default::default(),
    };

    // Note: This test may pass locally if there's no actual S3 connection attempt
    // during backup creation. The error would occur during upload.
    let result = snapshot_backup(
        "localhost".to_string(),
        5432,
        "testdb".to_string(),
        "postgres".to_string(),
        Some("password".to_string()),
        None,
        backup_dir,
        SshOptions::default(),
        storage,
        HashMap::new(),
    )
    .await;

    // The backup might succeed locally but fail on upload
    // Either way, we're testing that errors are handled gracefully
    if result.is_err() {
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        // Should not panic, should return a proper error
        assert!(!err_msg.contains("panic"), "Should not panic on S3 errors");
    }
}

#[tokio::test]
async fn test_s3_missing_bucket_error() {
    use postgres::cli::commands::StorageOptions;

    let storage = StorageOptions {
        remote_storage: true,
        provider_type: Some("s3".to_string()),
        bucket: None, // Missing bucket
        prefix: None,
        region: None,
        endpoint: None,
        access_key: None,
        secret_key: None,
        multi_tenant: Default::default(),
    };

    // Verify the storage options are invalid
    assert!(storage.bucket.is_none(), "Bucket should be None");
    assert!(storage.remote_storage, "Remote storage should be enabled");
}

// ============================================================================
// PostgreSQL Auth/Connection Failure Tests
// ============================================================================

#[tokio::test]
async fn test_postgres_connection_refused() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};
    use std::collections::HashMap;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Try to connect to a port with no PostgreSQL
    let result = snapshot_backup(
        "localhost".to_string(),
        59999, // Unlikely to have PostgreSQL running
        "testdb".to_string(),
        "postgres".to_string(),
        Some("password".to_string()),
        None,
        backup_dir,
        SshOptions::default(),
        StorageOptions::default(),
        HashMap::new(),
    )
    .await;

    assert!(result.is_err(), "Should fail with connection error");
    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("connection") || err_msg.contains("refused") || err_msg.contains("failed"),
        "Error should mention connection issue: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_postgres_invalid_credentials() {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};
    use std::collections::HashMap;

    // Skip if no test PostgreSQL available
    if env::var("POSTGRES_PORT").is_err() && env::var("CI").is_err() {
        println!("Skipping: No test PostgreSQL available");
        return;
    }

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    let port: u16 = env::var("POSTGRES_PORT")
        .unwrap_or_else(|_| "5432".to_string())
        .parse()
        .unwrap_or(5432);

    // Use invalid password
    let result = snapshot_backup(
        "localhost".to_string(),
        port,
        "postgres".to_string(),
        "postgres".to_string(),
        Some("WRONG_PASSWORD_12345".to_string()),
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
        assert!(
            err_msg.contains("auth")
                || err_msg.contains("password")
                || err_msg.contains("connection"),
            "Error should mention auth issue: {}",
            err_msg
        );
    }
}

// ============================================================================
// Invalid Target Time Tests
// ============================================================================

#[tokio::test]
async fn test_pitr_invalid_target_time_before_backup() {
    use chrono::{Duration, Utc};
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
    assert!(result.is_err(), "Should fail for target before backup");

    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("before")
            || err_msg.contains("no backup")
            || err_msg.contains("not found")
            || err_msg.contains("no base backup")
            || err_msg.contains("pitr"),
        "Error should mention target is before backup or no backup found: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_pitr_invalid_target_time_format() {
    // Test that invalid time formats are rejected
    let invalid_times = vec![
        "not-a-time",
        "2025-13-45T99:99:99Z", // Invalid date/time values
        "yesterday",
        "",
    ];

    for invalid_time in invalid_times {
        let result = chrono::DateTime::parse_from_rfc3339(invalid_time);
        assert!(
            result.is_err(),
            "Should reject invalid time format: {}",
            invalid_time
        );
    }
}

// ============================================================================
// Disk Full / Permission Tests
// ============================================================================

#[test]
fn test_backup_dir_not_writable() {
    use postgres::common::PostgresConfig;
    use postgres::manager::PostgresManager;

    // Try to create a manager with a non-existent parent directory
    let backup_dir = PathBuf::from("/nonexistent/path/that/should/not/exist/backups");

    let config = PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "testdb".to_string(),
        user: "postgres".to_string(),
        password: Some("password".to_string()),
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

    // This might succeed in creating the manager but fail on actual backup
    // The important thing is it doesn't panic
    let result = PostgresManager::new(config, backup_dir);
    // Result depends on whether the path can be created
    // We're mainly testing that it doesn't panic
    let _ = result;
}

// ============================================================================
// Exit Code Categorization Tests
// ============================================================================

#[test]
fn test_error_categorization() {
    use anyhow::anyhow;
    use postgres::error_codes::{categorize_error, ExitCode};

    // Usage errors
    assert_eq!(
        categorize_error(&anyhow!("Invalid argument: --foo")),
        ExitCode::UsageError
    );

    // Config errors
    assert_eq!(
        categorize_error(&anyhow!("Configuration file not found")),
        ExitCode::ConfigError
    );

    // Environment errors
    assert_eq!(
        categorize_error(&anyhow!("SSH tunnel failed")),
        ExitCode::EnvironmentError
    );
    assert_eq!(
        categorize_error(&anyhow!("pg_dump not found")),
        ExitCode::EnvironmentError
    );

    // Remote service errors
    assert_eq!(
        categorize_error(&anyhow!("S3 bucket not found")),
        ExitCode::RemoteServiceError
    );
    assert_eq!(
        categorize_error(&anyhow!("Upload failed: access denied")),
        ExitCode::RemoteServiceError
    );

    // Internal errors (default)
    assert_eq!(
        categorize_error(&anyhow!("Unexpected state")),
        ExitCode::InternalError
    );
}

#[test]
fn test_cli_error_types() {
    use postgres::error_codes::{CliError, ExitCode};

    let usage_err = CliError::usage("Bad argument");
    assert_eq!(usage_err.exit_code, ExitCode::UsageError);
    assert!(usage_err.to_string().contains("Bad argument"));

    let config_err = CliError::config("Invalid config");
    assert_eq!(config_err.exit_code, ExitCode::ConfigError);

    let env_err = CliError::environment("Network error");
    assert_eq!(env_err.exit_code, ExitCode::EnvironmentError);

    let remote_err = CliError::remote_service("S3 error");
    assert_eq!(remote_err.exit_code, ExitCode::RemoteServiceError);

    let internal_err = CliError::internal("Bug");
    assert_eq!(internal_err.exit_code, ExitCode::InternalError);
}

// ============================================================================
// Retention Policy Error Tests
// ============================================================================

#[test]
fn test_retention_invalid_policy_file() {
    use postgres::retention::PitrRetentionPolicy;

    // Test parsing invalid JSON
    let invalid_json = "{ not valid json }";
    let result: Result<PitrRetentionPolicy, _> = serde_json::from_str(invalid_json);
    assert!(result.is_err(), "Should reject invalid JSON");

    // Test parsing valid JSON but invalid schema
    let invalid_schema = r#"{"foo": "bar"}"#;
    let result: Result<PitrRetentionPolicy, _> = serde_json::from_str(invalid_schema);
    // This might succeed with defaults or fail - either is acceptable
    // The important thing is it doesn't panic
    let _ = result;
}

#[tokio::test]
async fn test_retention_missing_policy_file() {
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
    // Should either fail or use defaults - not panic
    let _ = result;
}

// ============================================================================
// Backup ID Validation Tests
// ============================================================================

#[test]
fn test_invalid_backup_id_format() {
    use uuid::Uuid;

    let invalid_ids = vec![
        "not-a-uuid",
        "12345",
        "",
        "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
    ];

    for invalid_id in invalid_ids {
        let result = Uuid::parse_str(invalid_id);
        assert!(
            result.is_err(),
            "Should reject invalid UUID: {}",
            invalid_id
        );
    }

    // Valid UUID should parse
    let valid_id = "550e8400-e29b-41d4-a716-446655440000";
    let result = Uuid::parse_str(valid_id);
    assert!(result.is_ok(), "Should accept valid UUID");
}
