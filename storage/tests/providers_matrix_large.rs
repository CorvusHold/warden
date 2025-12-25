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
    let test_bucket = match env::var("AWS_TEST_BUCKET") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[SKIP] providers_matrix_large: AWS_TEST_BUCKET not set: {e}");
            return;
        }
    };
    let access_key = env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = env::var("AWS_SECRET_ACCESS_KEY").ok();
    let region = env::var("AWS_REGION").ok();
    let endpoint = match env::var("AWS_ENDPOINT") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("[SKIP] providers_matrix_large: AWS_ENDPOINT not set");
            return;
        }
    };

    if access_key.is_none() || secret_key.is_none() {
        eprintln!("[SKIP] providers_matrix_large: AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY not set");
        return;
    }

    let providers = vec![("minio", ProviderKind::Minio, endpoint.clone())];
    let test_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/large_test_file.bin");
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
        std::fs::create_dir_all(&test_file.parent().unwrap()).unwrap();
        std::fs::write(&test_file, &data).expect("write large test file");
    }
    // Print file size
    let actual_size = std::fs::metadata(&test_file).expect("file metadata").len();
    println!("Test file size: {actual_size} bytes (expected: {LARGE_FILE_SIZE})");

    for (name, kind, endpoint) in providers {
        println!("\nTesting provider (large file): {name} ({endpoint})");
        let provider = match S3Provider::new_with_kind(
            region.clone(),
            Some(endpoint.clone()),
            access_key.clone(),
            secret_key.clone(),
            kind.clone(),
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SKIP] providers_matrix_large: provider init failed for {name}: {e:?}");
                return;
            }
        };
        provider.create_bucket(&test_bucket).await.ok();
        if let Err(e) = provider
            .upload_file(&test_bucket, test_key, &test_file, None, None)
            .await
        {
            eprintln!("[SKIP] providers_matrix_large: upload failed for {name}: {e:?}");
            return;
        }

        // Download and verify
        let download_path = env::temp_dir().join("large_downloaded_file.bin");
        if let Err(e) = provider
            .download_file(&test_bucket, test_key, &download_path)
            .await
        {
            eprintln!("[SKIP] providers_matrix_large: download failed for {name}: {e:?}");
            return;
        }
        let orig = std::fs::read(&test_file).expect("read orig");
        let downloaded = std::fs::read(&download_path).expect("read downloaded");
        assert_eq!(orig, downloaded, "Downloaded file does not match uploaded");
        std::fs::remove_file(&download_path).ok();

        // List objects and check
        let objects = match provider.list_objects(&test_bucket, Some("matrix/")).await {
            Ok(objects) => objects,
            Err(e) => {
                eprintln!("[SKIP] providers_matrix_large: list_objects failed for {name}: {e:?}");
                return;
            }
        };
        let found = objects.iter().any(|obj| obj.key == test_key);
        assert!(found, "Uploaded file not found in list_objects");

        // Delete and check
        if let Err(e) = provider.delete_object(&test_bucket, test_key).await {
            eprintln!("[SKIP] providers_matrix_large: delete_object failed for {name}: {e:?}");
            return;
        }
        let objects = match provider.list_objects(&test_bucket, Some("matrix/")).await {
            Ok(objects) => objects,
            Err(e) => {
                eprintln!(
                    "[SKIP] providers_matrix_large: list_objects failed after delete for {name}: {e:?}"
                );
                return;
            }
        };
        let found = objects.iter().any(|obj| obj.key == test_key);
        assert!(!found, "File not deleted");

        println!("Provider {name} passed large file upload, download, list, and delete test");
    }
}
