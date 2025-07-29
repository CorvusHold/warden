//! Provider matrix test for large files (multipart upload)

use std::env;
use std::path::PathBuf;
use storage::providers::{ProviderKind, S3Provider};
use storage::StorageProvider;

/// Size in bytes for the large test file (20MB)
const LARGE_FILE_SIZE: usize = 20 * 1024 * 1024;

#[tokio::test]
async fn test_provider_matrix_large_file() {
    let _ = env_logger::builder().is_test(true).try_init();
    let providers = vec![("minio", ProviderKind::Minio, "http://localhost:9000")];
    let test_bucket = env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "test-bucket".to_string());
    let test_file = PathBuf::from("testdata/large_test_file.bin");
    let test_key = "matrix/large_test_file.bin";

    // Generate a large file if it doesn't exist or if it's not exactly 20MB
    if !test_file.exists()
        || test_file.metadata().map(|m| m.len()).unwrap_or(0) != LARGE_FILE_SIZE as u64
    {
        let mut data = vec![0u8; LARGE_FILE_SIZE];
        // Fill with a pattern for verification
        for (i, v) in data.iter_mut().enumerate().take(LARGE_FILE_SIZE) {
            *v = (i % 256) as u8;
        }
        std::fs::create_dir_all("testdata").unwrap();
        std::fs::write(&test_file, &data).expect("write large test file");
    }
    // Print file size
    let actual_size = std::fs::metadata(&test_file).expect("file metadata").len();
    println!(
        "Test file size: {actual_size} bytes (expected: {LARGE_FILE_SIZE})"
    );

    for (name, kind, endpoint) in providers {
        println!("\nTesting provider (large file): {name} ({endpoint})");
        let provider = S3Provider::new_with_kind(
            Some("us-east-1".to_string()),
            Some(endpoint.to_string()),
            Some("root".to_string()),
            Some("password".to_string()),
            kind.clone(),
        )
        .await
        .expect("provider init");
        provider
            .upload_file(&test_bucket, test_key, &test_file, None, None)
            .await
            .map_err(|e| panic!("Upload failed for {name}: {e:?}"))
            .unwrap();

        // Download and verify
        let download_path = PathBuf::from("testdata/large_downloaded_file.bin");
        provider
            .download_file(&test_bucket, test_key, &download_path)
            .await
            .map_err(|e| panic!("Download failed for {name}: {e:?}"))
            .unwrap();
        let orig = std::fs::read(&test_file).expect("read orig");
        let downloaded = std::fs::read(&download_path).expect("read downloaded");
        assert_eq!(orig, downloaded, "Downloaded file does not match uploaded");
        std::fs::remove_file(&download_path).ok();

        // List objects and check
        let objects = provider
            .list_objects(&test_bucket, Some("matrix/"))
            .await
            .unwrap();
        let found = objects.iter().any(|obj| obj.key == test_key);
        assert!(found, "Uploaded file not found in list_objects");

        // Delete and check
        provider
            .delete_object(&test_bucket, test_key)
            .await
            .unwrap();
        let objects = provider
            .list_objects(&test_bucket, Some("matrix/"))
            .await
            .unwrap();
        let found = objects.iter().any(|obj| obj.key == test_key);
        assert!(!found, "File not deleted");

        println!("Provider {name} passed large file upload, download, list, and delete test");
    }
}
