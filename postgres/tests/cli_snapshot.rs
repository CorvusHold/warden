//! CLI smoke and real tests for `snapshot-backup` and `restore-snapshot` PostgreSQL subcommands
//! These tests check both argument parsing and actual side effects (where possible)

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;
use testcontainers::{clients, GenericImage};

fn warden_bin() -> Command {
    Command::cargo_bin("warden").expect("warden binary should build")
}

// --- Smoke Tests ---

#[test]
fn snapshot_backup_help_works() {
    let mut cmd = warden_bin();
    cmd.args(["postgresql", "snapshot-backup", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("snapshot-backup"));
}

#[test]
fn restore_snapshot_help_works() {
    let mut cmd = warden_bin();
    cmd.args(["postgresql", "restore-snapshot", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("restore-snapshot"));
}

// --- Real Tests (scaffold) ---

#[tokio::test]
async fn snapshot_backup_creates_backup_file() {
    // Setup: create a temp backup dir
    // Start a temporary Postgres container
    let docker = clients::Cli::default();
    let image = GenericImage::new("postgres", "16")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "testdb")
        .with_volume(
            "./postgres/tests/postgres-init-replication.sh",
            "/docker-entrypoint-initdb.d/init-replication.sh",
        );
    let node = docker.run(image);
    let host = "localhost";
    let port = node.get_host_port_ipv4(5432);
    let user = "postgres";
    let db = "testdb";

    // Wait for Postgres to be ready
    let mut ready = false;
    for i in 0..10 {
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

    // Ensure the test user exists
    let create_user_sql = "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'test') THEN CREATE ROLE test LOGIN PASSWORD 'test'; END IF; END $$;";
    let create_user = std::process::Command::new("psql")
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
            create_user_sql,
        ])
        .env("PGPASSWORD", "postgres")
        .output()
        .expect("Failed to create test user");
    assert!(
        create_user.status.success(),
        "Failed to create test user: {} {}",
        String::from_utf8_lossy(&create_user.stdout),
        String::from_utf8_lossy(&create_user.stderr)
    );

    // Setup: create a temp backup dir
    let backup_dir = tempdir().unwrap();
    let backup_dir_path = backup_dir.path().to_str().unwrap();

    // Run the CLI snapshot-backup command using the container's port
    let mut cmd = warden_bin();
    cmd.args([
        "postgresql",
        "snapshot-backup",
        "--host",
        host,
        "--port",
        &port.to_string(),
        "--user",
        "test",
        "--database",
        db,
        "--backup-dir",
        backup_dir_path,
    ]);
    let assert = cmd.assert();
    assert
        .success()
        .stdout(predicate::str::contains("snapshot"));
    let files: Vec<_> = fs::read_dir(backup_dir_path).unwrap().collect();
    assert!(!files.is_empty(), "No snapshot backup file was created");
}

#[test]
fn restore_snapshot_restores_data() {
    // Setup: create a temp backup dir and a fake backup file (scaffold)
    let backup_dir = tempdir().unwrap();
    let backup_dir_path = backup_dir.path().to_str().unwrap();
    // In a real test, you would create a valid snapshot backup file here
    // For now, just scaffold the command
    let mut cmd = warden_bin();
    cmd.args([
        "postgresql",
        "restore-snapshot",
        "--host",
        "localhost",
        "--user",
        "test",
        "--database",
        "testdb",
        "--backup-dir",
        backup_dir_path,
        "--backup-id",
        "some-backup-id",
        "--target-dir",
        backup_dir_path,
    ]);
    // Only check for failure or success for now
    let assert = cmd.assert();
    // You may want to check for a specific error message if no backup exists
    assert
        .failure()
        .stderr(predicate::str::contains("not found").or(predicate::str::contains("error")));
}
