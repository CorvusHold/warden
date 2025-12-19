//! Integration tests for Full Restore functionality
//!
//! These tests require Docker to be available and will spin up ephemeral
//! PostgreSQL containers to verify end-to-end restore functionality.
//!
//! Run with: cargo test --test full_restore_integration_test -- --ignored
//!
//! Environment variables:
//! - POSTGRES_TEST_IMAGE: PostgreSQL Docker image (default: postgres:15)
//! - SKIP_DOCKER_TESTS: Set to "1" to skip Docker-based tests

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tempfile::TempDir;

/// Check if Docker is available
fn docker_available() -> bool {
    if std::env::var("SKIP_DOCKER_TESTS").unwrap_or_default() == "1" {
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

/// PostgreSQL container for testing
struct PostgresContainer {
    container_id: String,
    #[allow(dead_code)]
    port: u16,
    #[allow(dead_code)]
    user: String,
    #[allow(dead_code)]
    password: String,
    #[allow(dead_code)]
    database: String,
}

impl PostgresContainer {
    /// Start a new PostgreSQL container
    fn start(port: u16, database: &str, user: &str, password: &str) -> Result<Self, String> {
        let image =
            std::env::var("POSTGRES_TEST_IMAGE").unwrap_or_else(|_| "postgres:15".to_string());

        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--rm",
                "-p",
                &format!("{}:5432", port),
                "-e",
                &format!("POSTGRES_USER={}", user),
                "-e",
                &format!("POSTGRES_PASSWORD={}", password),
                "-e",
                &format!("POSTGRES_DB={}", database),
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

    /// Wait for PostgreSQL to be ready
    fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            let status = Command::new("docker")
                .args(["exec", &self.container_id, "pg_isready", "-U", "postgres"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if status.map(|s| s.success()).unwrap_or(false) {
                return Ok(());
            }

            std::thread::sleep(Duration::from_millis(500));
        }

        Err("Timeout waiting for PostgreSQL to be ready".to_string())
    }

    /// Execute SQL in the container
    fn exec_sql(&self, sql: &str) -> Result<String, String> {
        let output = Command::new("docker")
            .args([
                "exec",
                &self.container_id,
                "psql",
                "-U",
                "postgres",
                "-c",
                sql,
            ])
            .output()
            .map_err(|e| format!("Failed to execute SQL: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "SQL execution failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Create a backup using pg_dump
    fn create_backup(&self, backup_dir: &PathBuf) -> Result<PathBuf, String> {
        let backup_id = format!("backup_{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
        let backup_path = backup_dir.join(&backup_id);
        std::fs::create_dir_all(&backup_path)
            .map_err(|e| format!("Failed to create backup dir: {}", e))?;

        let dump_file = backup_path.join("postgres.dump");

        // Use docker exec to run pg_dump and redirect to host file
        let output = Command::new("docker")
            .args([
                "exec",
                &self.container_id,
                "pg_dump",
                "-U",
                "postgres",
                "-Fc",
                "-f",
                "/tmp/backup.dump",
            ])
            .output()
            .map_err(|e| format!("Failed to run pg_dump: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "pg_dump failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Copy dump file from container
        let output = Command::new("docker")
            .args([
                "cp",
                &format!("{}:/tmp/backup.dump", self.container_id),
                dump_file.to_str().unwrap(),
            ])
            .output()
            .map_err(|e| format!("Failed to copy dump: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "Failed to copy dump: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        // Create metadata file
        let metadata = serde_json::json!({
            "backup_id": backup_id,
            "backup_type": "snapshot",
            "database": "postgres",
            "size_bytes": std::fs::metadata(&dump_file).map(|m| m.len()).unwrap_or(0),
            "start_time": chrono::Utc::now().to_rfc3339(),
            "server_version": "15.0"
        });

        let metadata_path = backup_path.join("backup_metadata.json");
        std::fs::write(&metadata_path, metadata.to_string())
            .map_err(|e| format!("Failed to write metadata: {}", e))?;

        Ok(backup_path)
    }
}

impl Drop for PostgresContainer {
    fn drop(&mut self) {
        // Stop and remove the container
        let _ = Command::new("docker")
            .args(["stop", &self.container_id])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
#[ignore = "Requires Docker"]
fn test_backup_and_restore_data_equality() {
    if !docker_available() {
        println!("Skipping test: Docker not available");
        return;
    }

    // Start source PostgreSQL container
    let source_port = 15432;
    let source = PostgresContainer::start(source_port, "testdb", "postgres", "password")
        .expect("Failed to start source container");

    source
        .wait_ready(Duration::from_secs(30))
        .expect("Source container not ready");

    // Create test data
    source
        .exec_sql("CREATE TABLE test_data (id SERIAL PRIMARY KEY, name TEXT, value INT);")
        .expect("Failed to create table");

    source
        .exec_sql("INSERT INTO test_data (name, value) VALUES ('item1', 100), ('item2', 200), ('item3', 300);")
        .expect("Failed to insert data");

    // Create backup
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_path = source
        .create_backup(&temp_dir.path().to_path_buf())
        .expect("Failed to create backup");

    println!("Backup created at: {:?}", backup_path);

    // Start target PostgreSQL container
    let target_port = 15433;
    let target = PostgresContainer::start(target_port, "testdb", "postgres", "password")
        .expect("Failed to start target container");

    target
        .wait_ready(Duration::from_secs(30))
        .expect("Target container not ready");

    // Copy backup to target container and restore
    let dump_file = backup_path.join("postgres.dump");

    // Copy dump to target container
    let output = Command::new("docker")
        .args([
            "cp",
            dump_file.to_str().unwrap(),
            &format!("{}:/tmp/backup.dump", target.container_id),
        ])
        .output()
        .expect("Failed to copy dump to target");

    assert!(output.status.success(), "Failed to copy dump to target");

    // Restore in target container
    let output = Command::new("docker")
        .args([
            "exec",
            &target.container_id,
            "pg_restore",
            "-U",
            "postgres",
            "-d",
            "testdb",
            "-c",
            "/tmp/backup.dump",
        ])
        .output()
        .expect("Failed to run pg_restore");

    // pg_restore may return non-zero even on success with warnings
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("error") || stderr.contains("FATAL") {
            panic!("pg_restore failed: {}", stderr);
        }
    }

    // Verify data in target
    let result = target
        .exec_sql("SELECT COUNT(*) FROM test_data;")
        .expect("Failed to query target");

    assert!(result.contains("3"), "Expected 3 rows, got: {}", result);

    let result = target
        .exec_sql("SELECT SUM(value) FROM test_data;")
        .expect("Failed to query sum");

    assert!(
        result.contains("600"),
        "Expected sum of 600, got: {}",
        result
    );

    println!("Data equality verified!");
}

#[test]
#[ignore = "Requires Docker"]
fn test_restore_to_new_database() {
    if !docker_available() {
        println!("Skipping test: Docker not available");
        return;
    }

    // Start PostgreSQL container
    let port = 15434;
    let container = PostgresContainer::start(port, "sourcedb", "postgres", "password")
        .expect("Failed to start container");

    container
        .wait_ready(Duration::from_secs(30))
        .expect("Container not ready");

    // Create test data in source database
    container
        .exec_sql("CREATE TABLE users (id SERIAL PRIMARY KEY, email TEXT);")
        .expect("Failed to create table");

    container
        .exec_sql("INSERT INTO users (email) VALUES ('user1@test.com'), ('user2@test.com');")
        .expect("Failed to insert data");

    // Create backup
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_path = container
        .create_backup(&temp_dir.path().to_path_buf())
        .expect("Failed to create backup");

    // Create new target database
    container
        .exec_sql("CREATE DATABASE targetdb;")
        .expect("Failed to create target database");

    // Copy dump to container and restore to new database
    let dump_file = backup_path.join("postgres.dump");

    let output = Command::new("docker")
        .args([
            "cp",
            dump_file.to_str().unwrap(),
            &format!("{}:/tmp/backup.dump", container.container_id),
        ])
        .output()
        .expect("Failed to copy dump");

    assert!(output.status.success());

    // Restore to new database
    let _output = Command::new("docker")
        .args([
            "exec",
            &container.container_id,
            "pg_restore",
            "-U",
            "postgres",
            "-d",
            "targetdb",
            "/tmp/backup.dump",
        ])
        .output()
        .expect("Failed to run pg_restore");

    // Verify data in new database
    let output = Command::new("docker")
        .args([
            "exec",
            &container.container_id,
            "psql",
            "-U",
            "postgres",
            "-d",
            "targetdb",
            "-c",
            "SELECT COUNT(*) FROM users;",
        ])
        .output()
        .expect("Failed to query target database");

    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("2"), "Expected 2 users, got: {}", result);

    println!("Restore to new database verified!");
}

#[test]
#[ignore = "Requires Docker"]
fn test_restore_replaces_existing_data() {
    if !docker_available() {
        println!("Skipping test: Docker not available");
        return;
    }

    // Start PostgreSQL container
    let port = 15435;
    let container = PostgresContainer::start(port, "testdb", "postgres", "password")
        .expect("Failed to start container");

    container
        .wait_ready(Duration::from_secs(30))
        .expect("Container not ready");

    // Create initial data
    container
        .exec_sql("CREATE TABLE items (id SERIAL PRIMARY KEY, name TEXT);")
        .expect("Failed to create table");

    container
        .exec_sql("INSERT INTO items (name) VALUES ('original1'), ('original2');")
        .expect("Failed to insert data");

    // Create backup of initial state
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_path = container
        .create_backup(&temp_dir.path().to_path_buf())
        .expect("Failed to create backup");

    // Modify data (simulate changes after backup)
    container
        .exec_sql("DELETE FROM items;")
        .expect("Failed to delete data");

    container
        .exec_sql("INSERT INTO items (name) VALUES ('modified1'), ('modified2'), ('modified3');")
        .expect("Failed to insert modified data");

    // Verify modified state
    let result = container
        .exec_sql("SELECT COUNT(*) FROM items;")
        .expect("Failed to query");
    assert!(result.contains("3"), "Expected 3 modified items");

    // Restore from backup
    let dump_file = backup_path.join("postgres.dump");

    let output = Command::new("docker")
        .args([
            "cp",
            dump_file.to_str().unwrap(),
            &format!("{}:/tmp/backup.dump", container.container_id),
        ])
        .output()
        .expect("Failed to copy dump");

    assert!(output.status.success());

    // Restore with clean option to replace existing data
    let _output = Command::new("docker")
        .args([
            "exec",
            &container.container_id,
            "pg_restore",
            "-U",
            "postgres",
            "-d",
            "testdb",
            "-c", // Clean (drop) database objects before recreating
            "/tmp/backup.dump",
        ])
        .output()
        .expect("Failed to run pg_restore");

    // Verify original data is restored
    let result = container
        .exec_sql("SELECT COUNT(*) FROM items;")
        .expect("Failed to query after restore");

    assert!(
        result.contains("2"),
        "Expected 2 original items after restore, got: {}",
        result
    );

    let result = container
        .exec_sql("SELECT name FROM items ORDER BY name;")
        .expect("Failed to query names");

    assert!(result.contains("original1"), "Missing original1");
    assert!(result.contains("original2"), "Missing original2");
    assert!(
        !result.contains("modified"),
        "Should not contain modified data"
    );

    println!("Replace existing data verified!");
}

#[test]
#[ignore = "Requires Docker"]
fn test_restore_health_check() {
    if !docker_available() {
        println!("Skipping test: Docker not available");
        return;
    }

    // Start PostgreSQL container
    let port = 15436;
    let container = PostgresContainer::start(port, "testdb", "postgres", "password")
        .expect("Failed to start container");

    container
        .wait_ready(Duration::from_secs(30))
        .expect("Container not ready");

    // Create and backup data
    container
        .exec_sql("CREATE TABLE health_test (id INT);")
        .expect("Failed to create table");

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_path = container
        .create_backup(&temp_dir.path().to_path_buf())
        .expect("Failed to create backup");

    // Restore
    let dump_file = backup_path.join("postgres.dump");

    Command::new("docker")
        .args([
            "cp",
            dump_file.to_str().unwrap(),
            &format!("{}:/tmp/backup.dump", container.container_id),
        ])
        .output()
        .expect("Failed to copy dump");

    Command::new("docker")
        .args([
            "exec",
            &container.container_id,
            "pg_restore",
            "-U",
            "postgres",
            "-d",
            "testdb",
            "-c",
            "/tmp/backup.dump",
        ])
        .output()
        .expect("Failed to restore");

    // Perform health check
    let output = Command::new("docker")
        .args([
            "exec",
            &container.container_id,
            "pg_isready",
            "-U",
            "postgres",
            "-d",
            "testdb",
        ])
        .output()
        .expect("Failed to run pg_isready");

    assert!(
        output.status.success(),
        "Database should be ready after restore"
    );

    // Verify we can query
    let result = container
        .exec_sql("SELECT 1 AS health_check;")
        .expect("Failed health check query");

    assert!(result.contains("1"), "Health check query should return 1");

    println!("Health check verified!");
}
