// use std::env;
// use std::path::{Path, PathBuf};
// use std::time::Duration;
// use tokio::fs;
// use uuid::Uuid;
// use storage::{StorageProviderType, StorageProviderFactory, StorageProvider};

// // Skip this test by default as it requires actual S3 credentials
// // Run with: cargo test --test postgres_integration_test -- --ignored
// #[tokio::test]
// #[ignore]
// async fn test_postgres_backup_integration() {
//     // Set up test environment
//     let test_id = Uuid::new_v4().to_string();
//     let bucket_name = env::var("TEST_BUCKET_NAME").unwrap_or_else(|_| "test-postgres-backups".to_string());
//     let backup_id = format!("test-backup-{}", test_id);
//     let temp_dir = std::env::temp_dir().join(format!("postgres-test-{}", test_id));

//     // Create test directories
//     let backup_dir = temp_dir.join("backup");
//     let restore_dir = temp_dir.join("restore");
//     fs::create_dir_all(&backup_dir).await.expect("Failed to create backup directory");
//     fs::create_dir_all(&restore_dir).await.expect("Failed to create restore directory");

//     // Create mock backup files
//     create_mock_backup_files(&backup_dir, &backup_id).await;

//     // Create storage provider
//     let provider = create_test_storage_provider(&bucket_name).await;

//     // Ensure the bucket exists
//     provider.create_bucket(&bucket_name).await.expect("Failed to create bucket");

//     // Upload backup
//     upload_backup(&provider, &backup_id, &backup_dir).await;

//     // List backups to verify upload
//     let objects = provider.list_objects(&bucket_name, Some(&format!("{}/", backup_id))).await.expect("Failed to list objects");
//     assert!(!objects.is_empty(), "No backup files found in storage");

//     // Generate pre-signed URL for metadata file
//     let url = provider.generate_presigned_url(
//         &bucket_name,
//         &format!("{}/backup_metadata.json", backup_id),
//         Duration::from_secs(3600)
//     ).await.expect("Failed to generate pre-signed URL");
//     assert!(!url.is_empty(), "Pre-signed URL should not be empty");

//     // Download backup
//     download_backup(&provider, &backup_id, &restore_dir).await;

//     // Verify downloaded files
//     verify_downloaded_files(&backup_dir, &restore_dir).await;

//     // Clean up
//     delete_backup(&provider, &backup_id).await;
//     fs::remove_dir_all(&temp_dir).await.expect("Failed to clean up test directories");

//     info!("PostgreSQL backup integration test completed successfully");
// }

// async fn create_mock_backup_files(backup_dir: &Path, backup_id: &str) {
//     // Create mock physical backup files
//     let base_backup_content = format!("Mock PostgreSQL base backup for {}", backup_id);
//     fs::write(backup_dir.join("base.tar.gz"), base_backup_content.clone()).await.expect("Failed to create mock base backup");

//     // Create pg_wal directory and mock WAL files
//     let pg_wal_dir = backup_dir.join("pg_wal");
//     fs::create_dir_all(&pg_wal_dir).await.expect("Failed to create pg_wal directory");

//     for i in 1..=3 {
//         let wal_content = format!("Mock WAL file {} for {}", i, backup_id);
//         fs::write(pg_wal_dir.join(format!("000000010000000{}.wal", i)), wal_content.clone())
//             .await
//             .expect("Failed to create mock WAL file");
//     }

//     // Create mock logical backup files
//     let sql_dump_content = format!("-- Mock SQL dump for {}\nCREATE TABLE test (id SERIAL PRIMARY KEY, name TEXT);", backup_id);
//     fs::write(backup_dir.join("pg_dump.sql"), sql_dump_content.clone()).await.expect("Failed to create mock SQL dump");

//     let custom_dump_content = format!("Mock custom format dump for {}", backup_id);
//     fs::write(backup_dir.join("pg_dump.dump"), custom_dump_content.clone()).await.expect("Failed to create mock custom dump");

//     // Create mock metadata file
//     let metadata_content = format!(
//         r#"{{
//             "backup_id": "{}",
//             "backup_type": "full",
//             "start_time": "2023-06-15T12:00:00Z",
//             "end_time": "2023-06-15T12:05:00Z",
//             "database_name": "test_db",
//             "database_version": "PostgreSQL 14.5",
//             "database_size": 104857600,
//             "wal_position": "0/1A2B3C4D",
//             "files": [
//                 {{ "name": "base.tar.gz", "size": {}, "checksum": "mock-checksum-1" }},
//                 {{ "name": "pg_dump.sql", "size": {}, "checksum": "mock-checksum-2" }},
//                 {{ "name": "pg_dump.dump", "size": {}, "checksum": "mock-checksum-3" }}
//             ]
//         }}"#,
//         backup_id,
//         base_backup_content.len(),
//         sql_dump_content.len(),
//         custom_dump_content.len()
//     );

//     fs::write(backup_dir.join("backup_metadata.json"), metadata_content)
//         .await
//         .expect("Failed to create mock metadata file");
// }

// async fn create_test_storage_provider() -> Box<dyn StorageProvider> {
//     // Create a storage provider for testing
//     StorageProviderFactory::create_s3_provider(
//         env::var("AWS_REGION").ok(),
//         env::var("AWS_ENDPOINT").ok(),
//         env::var("AWS_ACCESS_KEY_ID").ok(),
//         env::var("AWS_SECRET_ACCESS_KEY").ok(),
//     ).await.expect("Failed to create storage provider")
// }

// async fn upload_backup(provider: &Box<dyn StorageProvider>, backup_id: &str, backup_dir: &Path) {
//     let bucket_name = env::var("TEST_BUCKET_NAME").unwrap_or_else(|_| "test-postgres-backups".to_string());
//     info!("Uploading backup to storage...");

//     // Upload main backup files
//     let files = ["base.tar.gz", "pg_dump.sql", "pg_dump.dump", "backup_metadata.json"];
//     for file in &files {
//         let file_path = backup_dir.join(file);
//         let object_key = format!("{}/{}", backup_id, file);
//         let object_path = Path::new(&object_key);

//         provider.upload_file(
//             &bucket_name,
//             &object_key,
//             &file_path.to_string_lossy(),
//             None,
//             None
//         ).await.expect(&format!("Failed to upload {}", file));
//     }

//     // Upload WAL files
//     let pg_wal_dir = backup_dir.join("pg_wal");
//     let mut entries = fs::read_dir(&pg_wal_dir).await.expect("Failed to read pg_wal directory");

//     while let Some(entry) = entries.next_entry().await.expect("Failed to get directory entry") {
//         let path = entry.path();
//         if path.is_file() {
//             let file_name = path.file_name().unwrap().to_string_lossy().to_string();
//             let object_key = format!("{}/pg_wal/{}", backup_id, file_name);
//             let object_path = Path::new(&object_key);

//             provider.upload_file(
//                 &bucket_name,
//                 &object_key,
//                 &path.to_string_lossy(),
//                 None,
//                 None
//             ).await.expect(&format!("Failed to upload WAL file {}", file_name));
//         }
//     }

//     info!("Backup uploaded successfully");
// }

// async fn download_backup(provider: &Box<dyn StorageProvider>, backup_id: &str, restore_dir: &Path) {
//     let bucket_name = env::var("TEST_BUCKET_NAME").unwrap_or_else(|_| "test-postgres-backups".to_string());
//     info!("Downloading backup from storage...");

//     // List all objects for this backup
//     let prefix = format!("{}/", backup_id);
//     let objects = provider.list_objects(&bucket_name, Some(&prefix)).await.expect("Failed to list objects");

//     for obj in objects {
//         let target_path = Path::new(&obj.key);
//         let restore_path = restore_dir.join(target_path);

//         // Create parent directories if needed
//         if let Some(parent) = restore_path.parent() {
//             if !parent.exists() {
//                 fs::create_dir_all(parent).await.expect("Failed to create parent directory");
//             }
//         }

//         provider.download_file(
//             &bucket_name,
//             target_path,
//             &restore_path,
//         ).await.expect(&format!("Failed to download {}", obj.key));
//     }

//     info!("Backup downloaded successfully");
// }

// async fn verify_downloaded_files(backup_dir: &Path, restore_dir: &Path) {
//     // Verify main backup files
//     let files = ["base.tar.gz", "pg_dump.sql", "pg_dump.dump", "backup_metadata.json"];
//     for file in &files {
//         let original = fs::read_to_string(backup_dir.join(file)).await.expect(&format!("Failed to read original {}", file));
//         let restored = fs::read_to_string(restore_dir.join(file)).await.expect(&format!("Failed to read restored {}", file));

//         assert_eq!(original, restored, "Content mismatch for file {}", file);
//     }

//     // Verify WAL files
//     let original_wal_dir = backup_dir.join("pg_wal");
//     let restored_wal_dir = restore_dir.join("pg_wal");

//     let mut original_entries = fs::read_dir(&original_wal_dir).await.expect("Failed to read original pg_wal directory");

//     while let Some(entry) = original_entries.next_entry().await.expect("Failed to get directory entry") {
//         let path = entry.path();
//         if path.is_file() {
//             let file_name = path.file_name().unwrap().to_string_lossy().to_string();
//             let original = fs::read_to_string(&path).await.expect(&format!("Failed to read original WAL file {}", file_name));
//             let restored = fs::read_to_string(restored_wal_dir.join(&file_name))
//                 .await
//                 .expect(&format!("Failed to read restored WAL file {}", file_name));

//             assert_eq!(original, restored, "Content mismatch for WAL file {}", file_name);
//         }
//     }

//     info!("All files verified successfully");
// }

// async fn delete_backup(provider: &Box<dyn StorageProvider>, backup_id: &str) {
//     let bucket_name = env::var("TEST_BUCKET_NAME").unwrap_or_else(|_| "test-postgres-backups".to_string());
//     info!("Deleting backup from storage...");

//     // List all objects for this backup
//     let prefix = format!("{}/", backup_id);
//     let objects = provider.list_objects(&bucket_name, Some(&prefix)).await.expect("Failed to list objects");

//     for obj in objects {
//         provider.delete_object(&bucket_name, &obj.key).await.expect(&format!("Failed to delete {}", obj.key));
//     }

//     info!("Backup deleted successfully");
// }
