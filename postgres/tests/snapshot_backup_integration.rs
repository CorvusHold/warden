//! Integration tests for the snapshot backup feature.
//!
//! These tests require Docker to be running and use testcontainers for Postgres
//! and environment variables for MinIO/S3 configuration.
//!
//! Run with: `make test-ci` or manually with:
//! ```
//! AWS_ENDPOINT=http://localhost:9000 \
//! AWS_ACCESS_KEY_ID=minioadmin \
//! AWS_SECRET_ACCESS_KEY=minioadmin \
//! AWS_TEST_BUCKET=testbucket \
//! cargo test --package postgres snapshot_backup_integration -- --test-threads=1
//! ```

use assert_cmd::Command;
use std::env;
use tempfile::tempdir;
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

use storage::providers::aws::S3Provider;
use storage::StorageProvider;

/// Helper to wait for Postgres to be ready
async fn wait_for_postgres(host: &str, port: u16, user: &str, db: &str, password: &str) -> bool {
    for _ in 0..15 {
        let conn = std::process::Command::new("psql")
            .args([
                "-h",
                host,
                "-p",
                &port.to_string(),
                "-U",
                user,
                "-d",
                db,
                "-c",
                "SELECT 1;",
            ])
            .env("PGPASSWORD", password)
            .output();
        match &conn {
            Ok(out) if out.status.success() => return true,
            _ => std::thread::sleep(std::time::Duration::from_secs(1)),
        }
    }
    false
}

/// Test snapshot backup with labels and verify metadata is created
#[tokio::test]
async fn snapshot_backup_with_labels_creates_metadata() {
    // Start Postgres container
    let image = GenericImage::new("postgres", "16")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "testdb")
        .with_env_var("POSTGRES_LISTEN_ADDRESSES", "*");
    let node = image.start().await.unwrap();
    let host = "localhost";
    let port = node.get_host_port_ipv4(5432).await.unwrap();
    let user = "postgres";
    let db = "testdb";
    let password = "postgres";

    assert!(
        wait_for_postgres(host, port, user, db, password).await,
        "Postgres was not ready after waiting"
    );

    let backup_dir = tempdir().unwrap();
    let backup_dir_path = backup_dir.path().to_str().unwrap();

    // Run snapshot backup with labels
    let mut cmd = Command::new("cargo");
    cmd.args([
        "run",
        "-q",
        "-p",
        "warden",
        "--",
        "postgresql",
        "snapshot-backup",
        "--host",
        host,
        "--port",
        &port.to_string(),
        "--user",
        user,
        "--password",
        password,
        "--database",
        db,
        "--backup-dir",
        backup_dir_path,
        "--label",
        "env=test",
        "--label",
        "cluster=primary",
    ]);

    let output = cmd.output().expect("Failed to run snapshot-backup");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "snapshot-backup failed: status={} stdout={} stderr={}",
        output.status,
        stdout,
        stderr
    );

    // Verify output contains backup_id
    assert!(
        stdout.contains("backup_id="),
        "Output should contain backup_id=, got: {}",
        stdout
    );

    // Verify output contains local_path
    assert!(
        stdout.contains("local_path="),
        "Output should contain local_path=, got: {}",
        stdout
    );

    // Find the backup directory
    let entries: Vec<_> = std::fs::read_dir(backup_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir()
                && e.file_name()
                    .to_string_lossy()
                    .starts_with("snapshot_backup_")
        })
        .collect();

    assert_eq!(
        entries.len(),
        1,
        "Expected exactly one snapshot backup directory"
    );

    let backup_path = entries[0].path();

    // Verify metadata file exists
    let metadata_path = backup_path.join("backup_metadata.json");
    assert!(
        metadata_path.exists(),
        "Metadata file should exist at {:?}",
        metadata_path
    );

    // Parse and verify metadata content
    let metadata_content = std::fs::read_to_string(&metadata_path).unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&metadata_content).unwrap();

    assert_eq!(metadata["backup_type"], "snapshot");
    assert_eq!(metadata["database"], "testdb");
    assert!(metadata["backup_id"].as_str().is_some());
    assert!(metadata["start_time"].as_str().is_some());
    assert!(metadata["end_time"].as_str().is_some());
    assert!(metadata["size_bytes"].as_u64().is_some());
    assert_eq!(metadata["labels"]["env"], "test");
    assert_eq!(metadata["labels"]["cluster"], "primary");
    assert_eq!(metadata["created_by"], "warden");
    assert_eq!(metadata["version"], "1.0");

    // Verify dump file exists
    let dump_files: Vec<_> = std::fs::read_dir(&backup_path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "dump")
                .unwrap_or(false)
        })
        .collect();

    assert!(
        !dump_files.is_empty(),
        "At least one .dump file should exist in backup directory"
    );
}

/// Test snapshot backup with remote storage uploads metadata to S3
#[tokio::test]
async fn snapshot_backup_uploads_metadata_to_s3() {
    // Check if S3/MinIO is configured
    let endpoint = match env::var("AWS_ENDPOINT") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("[SKIP] snapshot_backup_uploads_metadata_to_s3: AWS_ENDPOINT not set");
            return;
        }
    };

    let bucket = env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "testbucket".to_string());
    let access_key = env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = env::var("AWS_SECRET_ACCESS_KEY").ok();
    let region = env::var("AWS_REGION").ok();

    // Start Postgres container
    let image = GenericImage::new("postgres", "16")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "metadb")
        .with_env_var("POSTGRES_LISTEN_ADDRESSES", "*");
    let node = image.start().await.unwrap();
    let host = "localhost";
    let port = node.get_host_port_ipv4(5432).await.unwrap();
    let user = "postgres";
    let db = "metadb";
    let password = "postgres";

    assert!(
        wait_for_postgres(host, port, user, db, password).await,
        "Postgres was not ready after waiting"
    );

    let prefix = format!("metadata-test-{}", uuid::Uuid::new_v4());
    let backup_dir = tempdir().unwrap();
    let backup_dir_path = backup_dir.path().to_str().unwrap();

    // Run snapshot backup with remote storage
    let mut cmd = Command::new("cargo");
    cmd.args([
        "run",
        "-q",
        "-p",
        "warden",
        "--",
        "postgresql",
        "snapshot-backup",
        "--host",
        host,
        "--port",
        &port.to_string(),
        "--user",
        user,
        "--password",
        password,
        "--database",
        db,
        "--backup-dir",
        backup_dir_path,
        "--remote-storage",
        "--storage-provider",
        "s3",
        "--storage-bucket",
        &bucket,
        "--storage-prefix",
        &prefix,
        "--storage-endpoint",
        &endpoint,
        "--label",
        "test=metadata",
    ]);

    if let Some(ref region) = region {
        cmd.args(["--storage-region", region]);
    }
    if let Some(ref access_key) = access_key {
        cmd.args(["--storage-access-key", access_key]);
    }
    if let Some(ref secret_key) = secret_key {
        cmd.args(["--storage-secret-key", secret_key]);
    }

    let output = cmd.output().expect("Failed to run snapshot-backup");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "snapshot-backup failed: status={} stdout={} stderr={}",
        output.status,
        stdout,
        stderr
    );

    // Verify remote_path is in output
    assert!(
        stdout.contains("remote_path="),
        "Output should contain remote_path= when using remote storage, got: {}",
        stdout
    );

    // Verify objects were uploaded to S3
    let provider = S3Provider::new(region, Some(endpoint), access_key, secret_key)
        .await
        .expect("Failed to create S3Provider");

    // Ensure bucket exists
    provider.create_bucket(&bucket).await.ok();

    let objects = provider
        .list_objects(&bucket, Some(&prefix))
        .await
        .expect("Failed to list objects");

    // Should have at least a dump file and metadata file
    let dump_objects: Vec<_> = objects
        .iter()
        .filter(|o| o.key.ends_with(".dump"))
        .collect();
    let metadata_objects: Vec<_> = objects
        .iter()
        .filter(|o| o.key.ends_with("backup_metadata.json"))
        .collect();

    assert!(
        !dump_objects.is_empty(),
        "Should have at least one .dump file in S3, objects: {:?}",
        objects.iter().map(|o| &o.key).collect::<Vec<_>>()
    );

    assert!(
        !metadata_objects.is_empty(),
        "Should have backup_metadata.json in S3, objects: {:?}",
        objects.iter().map(|o| &o.key).collect::<Vec<_>>()
    );
}

/// Test that snapshot backup fails gracefully with invalid SSH config
#[tokio::test]
async fn snapshot_backup_fails_with_invalid_ssh() {
    let backup_dir = tempdir().unwrap();
    let backup_dir_path = backup_dir.path().to_str().unwrap();

    let mut cmd = Command::new("cargo");
    cmd.args([
        "run",
        "-q",
        "-p",
        "warden",
        "--",
        "postgresql",
        "snapshot-backup",
        "--host",
        "localhost",
        "--port",
        "5432",
        "--user",
        "postgres",
        "--database",
        "postgres",
        "--backup-dir",
        backup_dir_path,
        "--ssh-host",
        "nonexistent.invalid.host",
        "--ssh-user",
        "testuser",
        "--ssh-key-path",
        "/nonexistent/key",
        "--ssh-remote-port",
        "5432",
    ]);

    let output = cmd.output().expect("Failed to run snapshot-backup");

    // Should fail with non-zero exit code
    assert!(
        !output.status.success(),
        "snapshot-backup should fail with invalid SSH config"
    );

    // Should have error message in stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("SSH") || stderr.contains("tunnel") || stderr.contains("failed"),
        "Error message should mention SSH/tunnel failure, got: {}",
        stderr
    );
}

/// Test that snapshot backup works without remote storage (local only)
#[tokio::test]
async fn snapshot_backup_local_only() {
    // Start Postgres container
    let image = GenericImage::new("postgres", "16")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "localdb")
        .with_env_var("POSTGRES_LISTEN_ADDRESSES", "*");
    let node = image.start().await.unwrap();
    let host = "localhost";
    let port = node.get_host_port_ipv4(5432).await.unwrap();
    let user = "postgres";
    let db = "localdb";
    let password = "postgres";

    assert!(
        wait_for_postgres(host, port, user, db, password).await,
        "Postgres was not ready after waiting"
    );

    let backup_dir = tempdir().unwrap();
    let backup_dir_path = backup_dir.path().to_str().unwrap();

    // Run snapshot backup without remote storage
    let mut cmd = Command::new("cargo");
    cmd.args([
        "run",
        "-q",
        "-p",
        "warden",
        "--",
        "postgresql",
        "snapshot-backup",
        "--host",
        host,
        "--port",
        &port.to_string(),
        "--user",
        user,
        "--password",
        password,
        "--database",
        db,
        "--backup-dir",
        backup_dir_path,
    ]);

    let output = cmd.output().expect("Failed to run snapshot-backup");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "snapshot-backup failed: status={} stdout={} stderr={}",
        output.status,
        stdout,
        stderr
    );

    // Should have backup_id and local_path but NOT remote_path
    assert!(stdout.contains("backup_id="));
    assert!(stdout.contains("local_path="));
    assert!(
        !stdout.contains("remote_path="),
        "Should not have remote_path when not using remote storage"
    );

    // Verify backup directory was created
    let entries: Vec<_> = std::fs::read_dir(backup_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_dir()
                && e.file_name()
                    .to_string_lossy()
                    .starts_with("snapshot_backup_")
        })
        .collect();

    assert!(!entries.is_empty(), "Backup directory should be created");
}
