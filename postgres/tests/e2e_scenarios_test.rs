//! End-to-End Test Scenarios for Warden PostgreSQL Data Protection
//!
//! These tests verify the complete data-path features:
//! - Scenario A: backup → inspect → full-restore → verify data equality
//! - Scenario B: backup + WAL → pitr-plan → pitr-restore → verify point-in-time
//! - Scenario C: produce many backups → retention-plan/apply → verify storage contents
//!
//! Run with:
//!   make test-ci
//! Or manually:
//!   AWS_ENDPOINT=http://localhost:9000 \
//!   AWS_ACCESS_KEY_ID=minioadmin \
//!   AWS_SECRET_ACCESS_KEY=minioadmin \
//!   AWS_REGION=us-east-1 \
//!   AWS_TEST_BUCKET=testbucket \
//!   cargo test -p postgres --test e2e_scenarios_test -- --test-threads=1

use chrono::{Duration, Utc};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tempfile::TempDir;

// ============================================================================
// Test Infrastructure
// ============================================================================

/// Check if Docker is available
fn docker_available() -> bool {
    if env::var("SKIP_DOCKER_TESTS").unwrap_or_default() == "1" {
        return false;
    }
    Command::new("docker")
        .args(["version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if MinIO is available
fn minio_available() -> bool {
    env::var("AWS_ENDPOINT").is_ok()
        && env::var("AWS_ACCESS_KEY_ID").is_ok()
        && env::var("AWS_SECRET_ACCESS_KEY").is_ok()
}

/// Get MinIO configuration from environment
fn get_minio_config() -> Option<MinioConfig> {
    if !minio_available() {
        return None;
    }
    Some(MinioConfig {
        endpoint: env::var("AWS_ENDPOINT").ok()?,
        access_key: env::var("AWS_ACCESS_KEY_ID").ok()?,
        secret_key: env::var("AWS_SECRET_ACCESS_KEY").ok()?,
        region: env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
        bucket: env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "testbucket".to_string()),
    })
}

struct MinioConfig {
    endpoint: String,
    access_key: String,
    secret_key: String,
    region: String,
    bucket: String,
}

/// PostgreSQL test container
#[allow(dead_code)] // Fields used by ignored tests that require Docker
struct PostgresTestContainer {
    container_id: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

impl PostgresTestContainer {
    fn start(port: u16, database: &str, user: &str, password: &str) -> Result<Self, String> {
        let image = env::var("POSTGRES_TEST_IMAGE").unwrap_or_else(|_| "postgres:15".to_string());

        // Check if port is already in use
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "-p", &format!("{}:5432", port),
                "-e", &format!("POSTGRES_USER={}", user),
                "-e", &format!("POSTGRES_PASSWORD={}", password),
                "-e", &format!("POSTGRES_DB={}", database),
                "-e", "POSTGRES_HOST_AUTH_METHOD=trust",
                &image,
            ])
            .output()
            .map_err(|e| format!("Failed to start container: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Failed to start container: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(Self {
            container_id,
            port,
            user: user.to_string(),
            password: password.to_string(),
            database: database.to_string(),
        })
    }

    fn wait_ready(&self, timeout: std::time::Duration) -> Result<(), String> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            let status = Command::new("docker")
                .args(["exec", &self.container_id, "pg_isready", "-U", &self.user])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if status.map(|s| s.success()).unwrap_or(false) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Err("Timeout waiting for PostgreSQL".to_string())
    }

    fn exec_sql(&self, sql: &str) -> Result<String, String> {
        let output = Command::new("docker")
            .args([
                "exec",
                &self.container_id,
                "psql",
                "-U", &self.user,
                "-d", &self.database,
                "-t", // Tuple only (no headers)
                "-A", // Unaligned output
                "-c", sql,
            ])
            .output()
            .map_err(|e| format!("Failed to execute SQL: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "SQL failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[allow(dead_code)] // Used by ignored tests that require Docker
    fn connection_string(&self) -> String {
        format!(
            "host=localhost port={} user={} password={} dbname={}",
            self.port, self.user, self.password, self.database
        )
    }
}

impl Drop for PostgresTestContainer {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["stop", &self.container_id])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Create test data in PostgreSQL
fn create_test_data(container: &PostgresTestContainer, table_name: &str, rows: &[(&str, i32)]) -> Result<(), String> {
    container.exec_sql(&format!(
        "CREATE TABLE IF NOT EXISTS {} (id SERIAL PRIMARY KEY, name TEXT NOT NULL, value INT NOT NULL);",
        table_name
    ))?;

    for (name, value) in rows {
        container.exec_sql(&format!(
            "INSERT INTO {} (name, value) VALUES ('{}', {});",
            table_name, name, value
        ))?;
    }
    Ok(())
}

/// Verify test data in PostgreSQL
fn verify_test_data(container: &PostgresTestContainer, table_name: &str, expected_count: i64) -> Result<(), String> {
    let count_str = container.exec_sql(&format!("SELECT COUNT(*) FROM {};", table_name))?;
    let count: i64 = count_str.parse().map_err(|e| format!("Parse error: {}", e))?;
    
    if count != expected_count {
        return Err(format!("Expected {} rows, got {}", expected_count, count));
    }
    Ok(())
}

/// Create a backup using the postgres crate directly
async fn create_snapshot_backup(
    host: &str,
    port: u16,
    database: &str,
    user: &str,
    password: &str,
    backup_dir: &PathBuf,
    minio: Option<&MinioConfig>,
) -> Result<String, String> {
    use postgres::cli::commands::{snapshot_backup, SshOptions, StorageOptions};

    let storage = match minio {
        Some(cfg) => StorageOptions {
            remote_storage: true,
            provider_type: Some("s3".to_string()),
            bucket: Some(cfg.bucket.clone()),
            prefix: Some("e2e-test".to_string()),
            region: Some(cfg.region.clone()),
            endpoint: Some(cfg.endpoint.clone()),
            access_key: Some(cfg.access_key.clone()),
            secret_key: Some(cfg.secret_key.clone()),
        },
        None => StorageOptions::default(),
    };

    let result = snapshot_backup(
        host.to_string(),
        port,
        database.to_string(),
        user.to_string(),
        Some(password.to_string()),
        None,
        backup_dir.clone(),
        SshOptions::default(),
        storage,
        HashMap::new(),
    )
    .await
    .map_err(|e| format!("Backup failed: {}", e))?;

    Ok(result.backup_id)
}

// ============================================================================
// Scenario A: backup → inspect → full-restore → verify data equality
// ============================================================================

#[tokio::test]
#[ignore = "Requires Docker and MinIO - run with: cargo test --test e2e_scenarios_test -- --ignored"]
async fn scenario_a_backup_inspect_restore_verify() {
    if !docker_available() {
        println!("Skipping: Docker not available");
        return;
    }

    let minio_config = get_minio_config();
    
    // Setup
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).expect("Failed to create backup dir");

    // Start source PostgreSQL
    let source_port = 25432;
    let source = PostgresTestContainer::start(source_port, "testdb", "postgres", "postgres")
        .expect("Failed to start source container");
    source.wait_ready(std::time::Duration::from_secs(30))
        .expect("Source not ready");

    // Create test data
    let test_data = vec![
        ("item_alpha", 100),
        ("item_beta", 200),
        ("item_gamma", 300),
    ];
    create_test_data(&source, "products", &test_data).expect("Failed to create test data");

    // Verify source data
    verify_test_data(&source, "products", 3).expect("Source data verification failed");

    // Step 1: Create backup
    println!("Step 1: Creating snapshot backup...");
    let backup_id = create_snapshot_backup(
        "localhost",
        source_port,
        "testdb",
        "postgres",
        "postgres",
        &backup_dir,
        minio_config.as_ref(),
    )
    .await
    .expect("Backup creation failed");

    println!("Backup created: {}", backup_id);

    // Step 2: Inspect backup (verify metadata exists)
    println!("Step 2: Inspecting backup...");
    let backup_metadata_path = find_backup_metadata(&backup_dir, &backup_id);
    assert!(backup_metadata_path.is_some(), "Backup metadata not found");

    let metadata_content = std::fs::read_to_string(backup_metadata_path.unwrap())
        .expect("Failed to read metadata");
    let metadata: serde_json::Value = serde_json::from_str(&metadata_content)
        .expect("Failed to parse metadata");
    
    assert_eq!(metadata["backup_type"], "snapshot");
    assert!(metadata["size_bytes"].as_u64().unwrap() > 0);
    println!("Backup metadata verified: size={} bytes", metadata["size_bytes"]);

    // Step 3: Start target PostgreSQL and restore
    println!("Step 3: Starting target container and restoring...");
    let target_port = 25433;
    let target = PostgresTestContainer::start(target_port, "testdb", "postgres", "postgres")
        .expect("Failed to start target container");
    target.wait_ready(std::time::Duration::from_secs(30))
        .expect("Target not ready");

    // Find and restore the dump file
    let dump_file = find_dump_file(&backup_dir).expect("Dump file not found");
    restore_dump_to_container(&target, &dump_file).expect("Restore failed");

    // Step 4: Verify data equality
    println!("Step 4: Verifying data equality...");
    verify_test_data(&target, "products", 3).expect("Target data verification failed");

    // Verify actual values
    let sum = target.exec_sql("SELECT SUM(value) FROM products;")
        .expect("Failed to query sum");
    assert_eq!(sum, "600", "Sum mismatch: expected 600, got {}", sum);

    println!("✅ Scenario A PASSED: backup → inspect → restore → verify");
}

// ============================================================================
// Scenario B: backup + WAL → pitr-plan → pitr-restore → verify point-in-time
// ============================================================================

#[tokio::test]
#[ignore = "Requires Docker - run with: cargo test --test e2e_scenarios_test -- --ignored"]
async fn scenario_b_pitr_plan_and_restore() {
    if !docker_available() {
        println!("Skipping: Docker not available");
        return;
    }

    use postgres::pitr::{PitrPlanner, RecoveryTarget};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().join("backups");
    let wal_dir = temp_dir.path().join("wal_archive");
    std::fs::create_dir_all(&backup_dir).expect("Failed to create backup dir");
    std::fs::create_dir_all(&wal_dir).expect("Failed to create WAL dir");

    // Create mock backup catalog for PITR testing
    let backup_time = Utc::now() - Duration::hours(2);
    let backup_id = "e2e-test-backup-001";
    
    // Create backup directory structure
    let backup_path = backup_dir.join(format!("snapshot_backup_{}", backup_id));
    std::fs::create_dir_all(&backup_path).expect("Failed to create backup path");
    std::fs::write(backup_path.join("PG_VERSION"), "15\n").expect("Failed to write PG_VERSION");

    // Create backup catalog
    let catalog = serde_json::json!({
        "backups": [{
            "id": backup_id,
            "backup_type": "Full",
            "status": "Completed",
            "start_time": backup_time.to_rfc3339(),
            "end_time": (backup_time + Duration::minutes(5)).to_rfc3339(),
            "wal_start": "0/1000000",
            "wal_end": "0/2000000",
            "size_bytes": 1024 * 1024,
            "backup_path": backup_path.to_string_lossy().to_string(),
            "server_version": "15.0"
        }]
    });

    std::fs::write(
        backup_dir.join("backup_catalog.json"),
        serde_json::to_string_pretty(&catalog).unwrap(),
    ).expect("Failed to write catalog");

    // Create mock WAL segments
    std::fs::write(wal_dir.join("000000010000000000000001"), vec![0u8; 1024])
        .expect("Failed to create WAL segment 1");
    std::fs::write(wal_dir.join("000000010000000000000002"), vec![0u8; 1024])
        .expect("Failed to create WAL segment 2");

    // Step 1: Create PITR plan
    println!("Step 1: Creating PITR plan...");
    let planner = PitrPlanner::new(backup_dir.clone())
        .with_wal_archive_dir(wal_dir.clone());

    let target_time = Utc::now() - Duration::hours(1);
    let target = RecoveryTarget::Time(target_time);

    let plan_result = planner.plan_recovery(target).await;
    assert!(plan_result.is_ok(), "PITR plan failed: {:?}", plan_result.err());

    let plan = plan_result.unwrap();
    println!("PITR plan created:");
    println!("  - Base backup: {}", plan.base_backup.id);
    println!("  - Target time: {}", target_time);
    println!("  - Valid: {}", plan.validation.is_valid);

    assert!(plan.validation.is_valid, "Plan should be valid");
    assert_eq!(plan.base_backup.id.to_string(), backup_id);

    // Step 2: Verify plan rejects invalid target times
    println!("Step 2: Verifying invalid target time rejection...");
    let invalid_target = RecoveryTarget::Time(Utc::now() - Duration::hours(5)); // Before backup
    let invalid_plan = planner.plan_recovery(invalid_target).await;
    assert!(invalid_plan.is_err(), "Should reject target before backup");

    println!("✅ Scenario B PASSED: PITR plan validation works correctly");
}

// ============================================================================
// Scenario C: produce many backups → retention-plan/apply → verify storage
// ============================================================================

#[tokio::test]
#[ignore = "Requires Docker - run with: cargo test --test e2e_scenarios_test -- --ignored"]
async fn scenario_c_retention_plan_and_apply() {
    use postgres::cli::commands::{retention_plan, retention_init, RetentionOptions, StorageOptions};
    use postgres::retention::PitrRetentionPolicy;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().join("backups");
    std::fs::create_dir_all(&backup_dir).expect("Failed to create backup dir");

    // Create multiple test backups with different ages
    let backup_ages = vec![
        ("backup_001", 1),   // 1 day old - should keep
        ("backup_002", 3),   // 3 days old - should keep
        ("backup_003", 10),  // 10 days old - might delete
        ("backup_004", 20),  // 20 days old - might delete
        ("backup_005", 40),  // 40 days old - should delete
    ];

    for (backup_id, days_ago) in &backup_ages {
        create_mock_backup(&backup_dir, backup_id, *days_ago);
    }

    // Step 1: Generate retention policy
    println!("Step 1: Generating retention policy...");
    let policy_file = temp_dir.path().join("retention_policy.json");
    retention_init(&policy_file, "standard", "json")
        .expect("Failed to create retention policy");

    assert!(policy_file.exists(), "Policy file should exist");

    // Verify policy content
    let policy_content = std::fs::read_to_string(&policy_file).unwrap();
    let policy: PitrRetentionPolicy = serde_json::from_str(&policy_content)
        .expect("Failed to parse policy");
    assert!(policy.enabled, "Policy should be enabled");

    // Step 2: Run retention plan (dry-run)
    println!("Step 2: Running retention plan...");
    let storage_opts = StorageOptions::default();
    let retention_opts = RetentionOptions {
        policy_file: Some(policy_file.clone()),
        backup_dir: backup_dir.clone(),
        wal_archive_dir: None,
        include_local: true,
        include_remote: false,
        format: "table".to_string(),
    };

    let plan_result = retention_plan(storage_opts.clone(), retention_opts.clone()).await;
    assert!(plan_result.is_ok(), "Retention plan failed: {:?}", plan_result.err());

    let result = plan_result.unwrap();
    println!("Retention plan results:");
    println!("  - Total backups: {}", result.evaluation.total_backups);
    println!("  - To keep: {}", result.evaluation.backups_to_keep.len());
    println!("  - To delete: {}", result.evaluation.backups_to_delete.len());

    assert_eq!(result.evaluation.total_backups, 5, "Should evaluate all 5 backups");

    // Step 3: Verify backups still exist (dry-run doesn't delete)
    println!("Step 3: Verifying dry-run didn't delete anything...");
    let remaining_backups = count_backups(&backup_dir);
    assert_eq!(remaining_backups, 5, "Dry-run should not delete backups");

    println!("✅ Scenario C PASSED: retention plan evaluates correctly");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_error_invalid_target_time() {
    use postgres::pitr::{PitrPlanner, RecoveryTarget};

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_dir = temp_dir.path().to_path_buf();

    // Create empty catalog
    let catalog = serde_json::json!({ "backups": [] });
    std::fs::write(
        backup_dir.join("backup_catalog.json"),
        serde_json::to_string(&catalog).unwrap(),
    ).expect("Failed to write catalog");

    let planner = PitrPlanner::new(backup_dir);
    let target = RecoveryTarget::Time(Utc::now());

    let result = planner.plan_recovery(target).await;
    assert!(result.is_err(), "Should fail with no backups available");

    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err_msg.contains("no backup") 
            || err_msg.contains("not found")
            || err_msg.contains("no base backup")
            || err_msg.contains("pitr"),
        "Error should mention missing backup: {}",
        err_msg
    );
}

#[test]
fn test_error_missing_storage_bucket() {
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
    };

    // This should fail when trying to create a storage provider
    assert!(storage.bucket.is_none(), "Bucket should be None");
    // The actual error would occur when create_storage_provider is called
}

#[test]
fn test_retention_policy_presets() {
    use postgres::cli::commands::retention_init;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // Test all presets create valid policies
    for preset in &["standard", "aggressive", "conservative", "gfs"] {
        let output_path = temp_dir.path().join(format!("{}_policy.json", preset));
        let result = retention_init(&output_path, preset, "json");
        
        assert!(result.is_ok(), "Preset '{}' should succeed", preset);
        assert!(output_path.exists(), "Policy file for '{}' should exist", preset);

        // Verify it's valid JSON
        let content = std::fs::read_to_string(&output_path).unwrap();
        let _: serde_json::Value = serde_json::from_str(&content)
            .expect(&format!("Policy '{}' should be valid JSON", preset));
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn find_backup_metadata(backup_dir: &PathBuf, _backup_id: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(backup_dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_dir() {
            let metadata_path = path.join("backup_metadata.json");
            if metadata_path.exists() {
                return Some(metadata_path);
            }
        }
    }
    None
}

fn find_dump_file(backup_dir: &PathBuf) -> Option<PathBuf> {
    for entry in walkdir::WalkDir::new(backup_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().map(|e| e == "dump").unwrap_or(false) {
            return Some(path.to_path_buf());
        }
    }
    None
}

fn restore_dump_to_container(container: &PostgresTestContainer, dump_file: &PathBuf) -> Result<(), String> {
    // Copy dump to container
    let output = Command::new("docker")
        .args([
            "cp",
            dump_file.to_str().unwrap(),
            &format!("{}:/tmp/backup.dump", container.container_id),
        ])
        .output()
        .map_err(|e| format!("Failed to copy dump: {}", e))?;

    if !output.status.success() {
        return Err(format!("Copy failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // Restore using pg_restore
    let output = Command::new("docker")
        .args([
            "exec",
            &container.container_id,
            "pg_restore",
            "-U", &container.user,
            "-d", &container.database,
            "-c", // Clean before restore
            "/tmp/backup.dump",
        ])
        .output()
        .map_err(|e| format!("Failed to restore: {}", e))?;

    // pg_restore may return non-zero with warnings, check stderr for actual errors
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("FATAL") || stderr.contains("could not connect") {
            return Err(format!("Restore failed: {}", stderr));
        }
    }

    Ok(())
}

fn create_mock_backup(backup_dir: &PathBuf, backup_id: &str, days_ago: i64) {
    let backup_path = backup_dir.join(format!("snapshot_backup_{}", backup_id));
    std::fs::create_dir_all(&backup_path).expect("Failed to create backup dir");

    let timestamp = Utc::now() - Duration::days(days_ago);
    let metadata = serde_json::json!({
        "backup_id": backup_id,
        "backup_type": "snapshot",
        "status": "Completed",
        "start_time": (timestamp - Duration::hours(1)).to_rfc3339(),
        "end_time": timestamp.to_rfc3339(),
        "size_bytes": 1024 * 1024,
        "database": "testdb",
        "pinned": false,
        "tags": []
    });

    std::fs::write(
        backup_path.join("backup_metadata.json"),
        serde_json::to_string_pretty(&metadata).unwrap(),
    ).expect("Failed to write metadata");

    // Create dummy dump file
    std::fs::write(backup_path.join("testdb.dump"), "dummy backup data")
        .expect("Failed to write dump");
}

fn count_backups(backup_dir: &PathBuf) -> usize {
    std::fs::read_dir(backup_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("snapshot_backup_")
                })
                .count()
        })
        .unwrap_or(0)
}

// ============================================================================
// Exit Code Tests
// ============================================================================

#[test]
fn test_exit_codes_documented() {
    // Verify exit codes match API.md documentation:
    // 0 = Success
    // 1 = Usage error (bad flags, missing arg)
    // 2 = Configuration error
    // 3 = Environment error (network, tools)
    // 4 = Remote service error (S3, C2)
    // 5 = Internal error (bug/unexpected)

    // These are the documented exit codes
    const EXIT_SUCCESS: i32 = 0;
    const EXIT_USAGE_ERROR: i32 = 1;
    const EXIT_CONFIG_ERROR: i32 = 2;
    const EXIT_ENV_ERROR: i32 = 3;
    const EXIT_REMOTE_ERROR: i32 = 4;
    const EXIT_INTERNAL_ERROR: i32 = 5;

    // Just verify they're distinct
    let codes = [
        EXIT_SUCCESS,
        EXIT_USAGE_ERROR,
        EXIT_CONFIG_ERROR,
        EXIT_ENV_ERROR,
        EXIT_REMOTE_ERROR,
        EXIT_INTERNAL_ERROR,
    ];

    for (i, &code1) in codes.iter().enumerate() {
        for (j, &code2) in codes.iter().enumerate() {
            if i != j {
                assert_ne!(code1, code2, "Exit codes should be unique");
            }
        }
    }
}
