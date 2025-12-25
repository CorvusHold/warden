use assert_cmd::Command;
use std::env;
use tempfile::tempdir;
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

use storage::providers::aws::S3Provider;
use storage::StorageProvider;

#[tokio::test]
async fn snapshot_backup_uploads_single_logical_dump_to_remote_storage() {
    let image = GenericImage::new("postgres", "16")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "postgres")
        .with_env_var("POSTGRES_LISTEN_ADDRESSES", "*");
    let node = image.start().await.unwrap();
    let host = "localhost";
    let port = node.get_host_port_ipv4(5432).await.unwrap();
    let user = "postgres";
    let db = "postgres";

    let mut ready = false;
    for _ in 0..10 {
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
            .env("PGPASSWORD", "postgres")
            .output();
        match &conn {
            Ok(out) if out.status.success() => {
                ready = true;
                break;
            }
            _ => std::thread::sleep(std::time::Duration::from_secs(1)),
        }
    }
    assert!(ready, "Postgres was not ready after waiting");

    let bucket = env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "test-bucket".to_string());
    let access_key = env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = env::var("AWS_SECRET_ACCESS_KEY").ok();
    let region = env::var("AWS_REGION").ok();
    let endpoint = match env::var("AWS_ENDPOINT") {
        Ok(v) => Some(v),
        Err(_) => {
            eprintln!(
                "[SKIP] snapshot_backup_uploads_single_logical_dump_to_remote_storage: AWS_ENDPOINT not set; skipping remote storage regression test",
            );
            return;
        }
    };

    let prefix = format!("snapshot-regression-{}", uuid::Uuid::new_v4());

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
        host,
        "--port",
        &port.to_string(),
        "--user",
        user,
        "--password",
        "postgres",
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
    ]);
    if let Some(ref region) = region {
        cmd.args(["--storage-region", region]);
    }
    if let Some(ref endpoint) = endpoint {
        cmd.args(["--storage-endpoint", endpoint]);
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

    let provider = S3Provider::new(region, endpoint, access_key, secret_key)
        .await
        .expect("Failed to create S3Provider");
    provider.create_bucket(&bucket).await.ok();

    let objects = match provider.list_objects(&bucket, Some(&prefix)).await {
        Ok(objects) => objects,
        Err(e) => {
            eprintln!(
                "[SKIP] snapshot_backup_uploads_single_logical_dump_to_remote_storage: failed to list objects in bucket {bucket} with prefix {prefix}: {e}",
            );
            return;
        }
    };

    let dump_objects: Vec<_> = objects
        .iter()
        .filter(|o| o.key.ends_with(".dump"))
        .collect();
    assert!(
        !dump_objects.is_empty(),
        "No .dump objects found under prefix {prefix} in bucket {bucket}; objects: {:?}",
        objects.iter().map(|o| o.key.clone()).collect::<Vec<_>>()
    );
    assert_eq!(
        dump_objects.len(),
        1,
        "Expected exactly one .dump object under prefix {prefix} in bucket {bucket}, found {}: {:?}",
        dump_objects.len(),
        dump_objects
            .iter()
            .map(|o| o.key.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        dump_objects[0].key.ends_with("/pg_dump.dump"),
        "Expected only pg_dump.dump under prefix {prefix}, found: {}",
        dump_objects[0].key
    );
}
