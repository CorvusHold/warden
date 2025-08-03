//! Test matrix for S3-compatible providers

use std::env;
use std::path::PathBuf;
use storage::providers::{ProviderKind, S3Provider};
use storage::StorageProvider;

#[tokio::test]
async fn test_provider_matrix() {
    let providers = vec![("minio", ProviderKind::Minio, "http://localhost:9000")];
    let test_bucket = env::var("AWS_TEST_BUCKET").unwrap_or_else(|_| "test-bucket".to_string());
    let test_file = PathBuf::from("testdata/test_file.txt");
    let test_key = "matrix/test_file.txt";

    for (name, kind, endpoint) in providers {
        println!("\nTesting provider: {name} ({endpoint})");
        let provider = S3Provider::new_with_kind(
            Some("us-east-1".to_string()),
            Some(endpoint.to_string()),
            Some("root".to_string()),
            Some("password".to_string()),
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
        let download_path = PathBuf::from("testdata/downloaded_file.txt");
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
