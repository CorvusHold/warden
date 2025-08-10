//! Test matrix for S3-compatible providers

use std::env;
use std::path::PathBuf;
use storage::providers::{ProviderKind, S3Provider};
use storage::StorageProvider;

#[tokio::test]
async fn test_provider_matrix() {
    // Read configuration from environment to match CI settings
    let endpoint = env::var("AWS_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string());
    let access_key = env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key = env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
    let region = env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());

    let providers = vec![("minio", ProviderKind::Minio, endpoint.as_str())];
    let test_bucket = env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "test-bucket".to_string());
    // Make path robust relative to the crate directory
    let test_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/test_file.txt");
    let test_key = "matrix/test_file.txt";

    for (name, kind, endpoint) in providers {
        println!("\nTesting provider: {name} ({endpoint})");
        let provider = S3Provider::new_with_kind(
            Some(region.clone()),
            Some(endpoint.to_string()),
            Some(access_key.clone()),
            Some(secret_key.clone()),
            kind.clone(),
        )
        .await
        .expect("provider init");
        provider.create_bucket(&test_bucket).await.ok();
        provider
            .upload_file(&test_bucket, test_key, &test_file, None, None)
            .await
            .map_err(|e| panic!("Upload failed for {name}: {e:?}"))
            .unwrap();

        // Download and verify
        let download_path = std::env::temp_dir().join("downloaded_file.txt");
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

        println!("Provider {name} passed upload, download, list, and delete test");
    }
}
