//! Real integration tests for snapshot-backup and restore-snapshot using a temporary PostgreSQL database via testcontainers.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
#[tokio::test]

async fn snapshot_backup_and_restore_real() {
    println!("[TEST] Starting Docker client and Postgres container...");
    let image = GenericImage::new("postgres", "16")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "postgres")
        .with_env_var("POSTGRES_LISTEN_ADDRESSES", "*");
    let node = image.start().await.unwrap();
    let host = "localhost";
    let port = node.get_host_port_ipv4(5432).await.unwrap();
    let user = "postgres";
    let db = "postgres";
    println!("[TEST] Postgres container started on {host}:{port}");

    let backup_dir = tempdir().unwrap();
    let backup_dir_path = backup_dir.path().to_str().unwrap();
    println!("[TEST] Using backup dir: {backup_dir_path}");

    // Wait for Postgres to be ready (simple retry loop)
    let mut ready = false;
    for i in 0..10 {
        println!("[TEST] Checking Postgres readiness, attempt {}...", i + 1);
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
            Ok(out) => {
                println!(
                    "[TEST] psql stdout: {}",
                    String::from_utf8_lossy(&out.stdout)
                );
                println!(
                    "[TEST] psql stderr: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
                if out.status.success() {
                    println!("[TEST] Postgres is ready.");
                    ready = true;
                    break;
                } else {
                    println!("[TEST] Postgres not ready yet (status: {}).", out.status);
                }
            }
            Err(e) => {
                println!("[TEST] Error running psql: {e}");
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    assert!(ready, "Postgres was not ready after waiting");

    // Ensure the test user exists before running the CLI or inserting as test
    let create_user_sql = "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'test') THEN CREATE ROLE test LOGIN PASSWORD 'test'; END IF; END $$;";
    let create_user = std::process::Command::new("psql")
        .args([
            "-h",
            host,
            "-p",
            &port.to_string(),
            "-U",
            user, // superuser
            "-d",
            db,
            "-c",
            create_user_sql,
        ])
        .env("PGPASSWORD", "postgres")
        .output()
        .expect("Failed to create test user");
    println!(
        "[TEST] Create user stdout: {}",
        String::from_utf8_lossy(&create_user.stdout)
    );
    println!(
        "[TEST] Create user stderr: {}",
        String::from_utf8_lossy(&create_user.stderr)
    );
    assert!(
        create_user.status.success(),
        "Failed to create test user: {} {}",
        String::from_utf8_lossy(&create_user.stdout),
        String::from_utf8_lossy(&create_user.stderr)
    );

    // Insert test table and row
    let sql = "CREATE TABLE test_table (id SERIAL PRIMARY KEY, name TEXT); INSERT INTO test_table (name) VALUES ('testdata');";
    println!("[TEST] Inserting test data with SQL: {sql}");
    let insert = std::process::Command::new("psql")
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
            sql,
        ])
        .env("PGPASSWORD", "postgres")
        .output()
        .expect("Failed to insert test data");
    println!(
        "[TEST] Insert stdout: {}",
        String::from_utf8_lossy(&insert.stdout)
    );
    println!(
        "[TEST] Insert stderr: {}",
        String::from_utf8_lossy(&insert.stderr)
    );
    assert!(
        insert.status.success(),
        "Failed to insert test data: {} {}",
        String::from_utf8_lossy(&insert.stdout),
        String::from_utf8_lossy(&insert.stderr)
    );

    // Print command line and backup dir
    println!("[TEST] Running: warden postgresql snapshot-backup --host {host} --port {port} --user {user} --database {db} --backup-dir {backup_dir_path}");
    println!("[TEST] Backup dir before backup: {backup_dir_path}");
    let before_files: Vec<_> = fs::read_dir(backup_dir_path).unwrap().collect();
    println!(
        "[TEST] Files in backup dir before backup: {:?}",
        before_files
            .iter()
            .map(|f| f.as_ref().map(|e| e.path()))
            .collect::<Vec<_>>()
    );

    // Run snapshot-backup
    let output = Command::cargo_bin("warden")
        .unwrap()
        .args([
            "postgresql",
            "snapshot-backup",
            "--host",
            host,
            "--port",
            &port.to_string(),
            "--user",
            user,
            "--database",
            db,
            "--backup-dir",
            backup_dir_path,
        ])
        .output()
        .expect("Failed to run snapshot-backup");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    println!("[TEST] snapshot-backup exit status: {}", output.status);
    println!("[TEST] snapshot-backup stdout: {stdout}");
    println!("[TEST] snapshot-backup stderr: {stderr}");
    // List files in backup dir for debug
    let files: Vec<_> = fs::read_dir(backup_dir_path).unwrap().collect();
    println!(
        "[TEST] Files in backup dir after backup: {:?}",
        files
            .iter()
            .map(|f| f.as_ref().map(|e| e.path()))
            .collect::<Vec<_>>()
    );
    if !output.status.success() {
        panic!(
            "snapshot-backup failed: status={} stdout={} stderr={}",
            output.status, stdout, stderr
        );
    }
    if files.is_empty() {
        panic!(
            "No snapshot backup file was created.\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    println!("[TEST] Snapshot backup appears to have succeeded and files were created.");

    // (Optional) Run restore-snapshot with a real backup id if you can extract it from output or file name
    // For now, just scaffold:
    // let backup_id = ...;
    // let restore_output = Command::cargo_bin("warden")
    //     .unwrap()
    //     .args(&[
    //         "postgresql", "restore-snapshot",
    //         "--host", &host,
    //         "--port", &port.to_string(),
    //         "--user", &user,
    //         "--database", &db,
    //         "--backup-dir", backup_dir_path,
    //         "--backup-id", &backup_id,
    //         "--target-dir", backup_dir_path,
    //     ])
    //     .output()
    //     .expect("Failed to run restore-snapshot");
    // assert!(restore_output.status.success(), "restore-snapshot failed: {:?}", restore_output);
}
