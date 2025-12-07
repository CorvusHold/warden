//! Integration tests for backup retention with MinIO/S3.
//!
//! These tests require a running MinIO instance. They can be run with:
//!   AWS_ENDPOINT=http://localhost:9000 \
//!   AWS_ACCESS_KEY_ID=minioadmin \
//!   AWS_SECRET_ACCESS_KEY=minioadmin \
//!   AWS_REGION=us-east-1 \
//!   AWS_TEST_BUCKET=test-retention \
//!   cargo test -p postgres --test retention_integration_test

use chrono::{Duration, Utc};
use std::env;
use std::path::PathBuf;
use tempfile::TempDir;

/// Check if MinIO/S3 is available for testing
fn minio_available() -> bool {
    env::var("AWS_ENDPOINT").is_ok()
        && env::var("AWS_ACCESS_KEY_ID").is_ok()
        && env::var("AWS_SECRET_ACCESS_KEY").is_ok()
}

/// Get test configuration from environment
fn get_test_config() -> Option<TestConfig> {
    if !minio_available() {
        return None;
    }

    Some(TestConfig {
        endpoint: env::var("AWS_ENDPOINT").ok(),
        access_key: env::var("AWS_ACCESS_KEY_ID").ok(),
        secret_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
        region: env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
        bucket: env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "test-retention".to_string()),
    })
}

struct TestConfig {
    endpoint: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    region: String,
    bucket: String,
}

/// Create a test backup directory with metadata
fn create_test_backup_dir(
    base_dir: &PathBuf,
    backup_id: &str,
    backup_type: &str,
    days_ago: i64,
) -> PathBuf {
    let backup_dir = base_dir.join(format!("{}_backup_{}", backup_type, backup_id));
    std::fs::create_dir_all(&backup_dir).unwrap();

    let now = Utc::now();
    let timestamp = now - Duration::days(days_ago);

    let metadata = serde_json::json!({
        "backup_id": backup_id,
        "backup_type": backup_type,
        "status": "Completed",
        "start_time": (timestamp - Duration::hours(1)).to_rfc3339(),
        "end_time": timestamp.to_rfc3339(),
        "size_bytes": 1024 * 1024 * 10, // 10 MB
        "database": "testdb",
        "pinned": false,
        "tags": []
    });

    let metadata_path = backup_dir.join("backup_metadata.json");
    std::fs::write(&metadata_path, serde_json::to_string_pretty(&metadata).unwrap()).unwrap();

    // Create a dummy data file
    let data_path = backup_dir.join("data.dump");
    std::fs::write(&data_path, "dummy backup data").unwrap();

    backup_dir
}

/// Create a test WAL segment file
fn create_test_wal_segment(wal_dir: &PathBuf, segment_name: &str, days_ago: i64) {
    std::fs::create_dir_all(wal_dir).unwrap();
    let segment_path = wal_dir.join(segment_name);
    std::fs::write(&segment_path, "dummy wal data").unwrap();

    // Set modification time (approximate - actual mtime setting would need platform-specific code)
    // For testing, we rely on the metadata timestamp instead
}

// ============================================================================
// Local Retention Tests
// ============================================================================

#[test]
fn test_local_retention_plan() {
    use postgres::cli::commands::{retention_plan, RetentionOptions, StorageOptions};
    use postgres::retention::PitrRetentionPolicy;

    let temp_dir = TempDir::new().unwrap();
    let backup_dir = temp_dir.path().to_path_buf();

    // Create test backups
    create_test_backup_dir(&backup_dir, "backup1", "full", 30);
    create_test_backup_dir(&backup_dir, "backup2", "full", 15);
    create_test_backup_dir(&backup_dir, "backup3", "full", 5);
    create_test_backup_dir(&backup_dir, "backup4", "full", 1);

    // Create a policy file
    let policy = PitrRetentionPolicy::default();
    let policy_file = temp_dir.path().join("policy.json");
    std::fs::write(&policy_file, serde_json::to_string_pretty(&policy).unwrap()).unwrap();

    let storage_opts = StorageOptions {
        remote_storage: false,
        provider_type: None,
        bucket: None,
        prefix: None,
        region: None,
        endpoint: None,
        access_key: None,
        secret_key: None,
    };

    let retention_opts = RetentionOptions {
        policy_file: Some(policy_file),
        backup_dir,
        wal_archive_dir: None,
        include_local: true,
        include_remote: false,
        format: "table".to_string(),
    };

    // Run retention plan
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(retention_plan(storage_opts, retention_opts));

    assert!(result.is_ok());
    let plan_result = result.unwrap();

    // Should have evaluated 4 backups
    assert_eq!(plan_result.evaluation.total_backups, 4);

    // With default policy, some should be kept and some deleted
    assert!(!plan_result.evaluation.backups_to_keep.is_empty());
}

#[test]
fn test_retention_init_creates_policy() {
    use postgres::cli::commands::retention_init;

    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test_policy.json");

    // Test standard preset
    let result = retention_init(&output_path, "standard", "json");
    assert!(result.is_ok());
    assert!(output_path.exists());

    // Verify the policy is valid JSON
    let content = std::fs::read_to_string(&output_path).unwrap();
    let policy: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(policy["version"], "1.0");
    assert_eq!(policy["enabled"], true);
}

#[test]
fn test_retention_init_presets() {
    use postgres::cli::commands::retention_init;

    let temp_dir = TempDir::new().unwrap();

    // Test all presets
    for preset in &["standard", "aggressive", "conservative", "gfs"] {
        let output_path = temp_dir.path().join(format!("{}_policy.json", preset));
        let result = retention_init(&output_path, preset, "json");
        assert!(result.is_ok(), "Failed for preset: {}", preset);
        assert!(output_path.exists(), "File not created for preset: {}", preset);
    }
}

#[test]
fn test_retention_init_yaml_format() {
    use postgres::cli::commands::retention_init;

    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test_policy.yaml");

    let result = retention_init(&output_path, "standard", "yaml");
    assert!(result.is_ok());
    assert!(output_path.exists());

    // Verify the policy is valid YAML
    let content = std::fs::read_to_string(&output_path).unwrap();
    let policy: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
    assert!(policy["version"].as_str().is_some());
}

#[test]
fn test_format_retention_plan_json() {
    use postgres::cli::commands::{format_retention_plan, RetentionPlanResult};
    use postgres::retention::RetentionResult;

    let result = RetentionPlanResult {
        evaluation: RetentionResult::new(),
        policy_source: "test".to_string(),
        includes_local: true,
        includes_remote: false,
    };

    let output = format_retention_plan(&result, "json");
    assert!(output.contains("timestamp"));
    assert!(output.contains("total_backups"));
}

#[test]
fn test_format_retention_plan_table() {
    use postgres::cli::commands::{format_retention_plan, RetentionPlanResult};
    use postgres::retention::RetentionResult;

    let result = RetentionPlanResult {
        evaluation: RetentionResult::new(),
        policy_source: "test".to_string(),
        includes_local: true,
        includes_remote: false,
    };

    let output = format_retention_plan(&result, "table");
    assert!(output.contains("Retention Plan"));
    assert!(output.contains("Policy source"));
}

// ============================================================================
// Remote Storage Tests (require MinIO)
// ============================================================================

#[test]
#[ignore = "Requires MinIO - run with: cargo test --test retention_integration_test -- --ignored"]
fn test_remote_retention_plan() {
    let config = match get_test_config() {
        Some(c) => c,
        None => {
            eprintln!("Skipping test: MinIO not configured");
            return;
        }
    };

    use postgres::cli::commands::{retention_plan, RetentionOptions, StorageOptions};

    let temp_dir = TempDir::new().unwrap();
    let backup_dir = temp_dir.path().to_path_buf();

    let storage_opts = StorageOptions {
        remote_storage: true,
        provider_type: Some("s3".to_string()),
        bucket: Some(config.bucket),
        prefix: Some("retention-test".to_string()),
        region: Some(config.region),
        endpoint: config.endpoint,
        access_key: config.access_key,
        secret_key: config.secret_key,
    };

    let retention_opts = RetentionOptions {
        policy_file: None,
        backup_dir,
        wal_archive_dir: None,
        include_local: false,
        include_remote: true,
        format: "table".to_string(),
    };

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(retention_plan(storage_opts, retention_opts));

    // Should succeed even if no backups exist
    assert!(result.is_ok());
}

// ============================================================================
// WAL Inventory Tests
// ============================================================================

#[test]
fn test_wal_inventory_scan() {
    use postgres::retention::WalInventory;

    let temp_dir = TempDir::new().unwrap();
    let wal_dir = temp_dir.path().join("wal_archive");
    std::fs::create_dir_all(&wal_dir).unwrap();

    // Create some WAL segment files
    std::fs::write(wal_dir.join("000000010000000000000001"), "wal1").unwrap();
    std::fs::write(wal_dir.join("000000010000000000000002"), "wal2").unwrap();
    std::fs::write(wal_dir.join("000000010000000000000003.partial"), "wal3").unwrap();

    let inventory = WalInventory::scan_local_directory(&wal_dir).unwrap();

    assert_eq!(inventory.segments.len(), 3);
    assert!(inventory.segments.iter().any(|s| s.segment_id == 1));
    assert!(inventory.segments.iter().any(|s| s.segment_id == 2));
    assert!(inventory.segments.iter().any(|s| s.is_partial));
}

#[test]
fn test_wal_segment_parsing() {
    use postgres::retention::{BackupLocation, WalSegment};

    // Standard segment
    let seg = WalSegment::from_filename(
        "000000010000000000000001",
        16 * 1024 * 1024,
        BackupLocation::Local("/wal/1".to_string()),
    );
    assert!(seg.is_some());
    let seg = seg.unwrap();
    assert_eq!(seg.timeline, 1);
    assert_eq!(seg.segment_id, 1);
    assert!(!seg.is_partial);

    // Partial segment
    let seg = WalSegment::from_filename(
        "000000010000000000000002.partial",
        8 * 1024 * 1024,
        BackupLocation::Local("/wal/2".to_string()),
    );
    assert!(seg.is_some());
    assert!(seg.unwrap().is_partial);

    // Compressed segment
    let seg = WalSegment::from_filename(
        "000000010000000000000003.gz",
        4 * 1024 * 1024,
        BackupLocation::Local("/wal/3".to_string()),
    );
    assert!(seg.is_some());
    assert_eq!(seg.unwrap().segment_id, 3);

    // Invalid name
    let seg = WalSegment::from_filename(
        "not_a_wal_segment",
        1024,
        BackupLocation::Local("/wal/invalid".to_string()),
    );
    assert!(seg.is_none());
}

// ============================================================================
// Policy Conversion Tests
// ============================================================================

#[test]
fn test_policy_serialization_roundtrip() {
    use postgres::retention::PitrRetentionPolicy;

    let original = PitrRetentionPolicy::gfs_standard();

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&original).unwrap();

    // Deserialize back
    let restored: PitrRetentionPolicy = serde_json::from_str(&json).unwrap();

    assert_eq!(original.version, restored.version);
    assert_eq!(original.enabled, restored.enabled);
    assert_eq!(original.rules.len(), restored.rules.len());
}

#[test]
fn test_policy_yaml_serialization() {
    use postgres::retention::PitrRetentionPolicy;

    let original = PitrRetentionPolicy::conservative();

    // Serialize to YAML
    let yaml = serde_yaml::to_string(&original).unwrap();

    // Deserialize back
    let restored: PitrRetentionPolicy = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(original.version, restored.version);
    assert_eq!(original.enabled, restored.enabled);
}
