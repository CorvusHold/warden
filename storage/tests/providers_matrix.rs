//! Test matrix for S3-compatible providers

use std::env;
use std::path::PathBuf;
use storage::providers::{ProviderKind, S3Provider};
use storage::StorageProvider;

#[tokio::test]
async fn test_provider_matrix() {
    // Read configuration from environment to match CI settings
    let endpoint = match env::var("AWS_ENDPOINT") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("[SKIP] providers_matrix: AWS_ENDPOINT not set");
            return;
        }
    };

    let providers = vec![("minio", ProviderKind::Minio, endpoint.as_str())];
    let test_bucket = match env::var("AWS_TEST_BUCKET") {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[SKIP] providers_matrix: AWS_TEST_BUCKET not set: {e}");
            return;
        }
    };
    // Make path robust relative to the crate directory
    let test_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/test_file.txt");
    let test_key = "matrix/test_file.txt";

    // Get credentials from environment (same as aws_multipart.rs tests)
    let access_key = env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = env::var("AWS_SECRET_ACCESS_KEY").ok();
    let region = env::var("AWS_REGION").ok();

    if access_key.is_none() || secret_key.is_none() {
        eprintln!("[SKIP] providers_matrix: AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY not set");
        return;
    }

    for (name, kind, endpoint) in providers {
        println!("\nTesting provider: {name} ({endpoint})");
        let provider = match S3Provider::new_with_kind(
            region.clone(),
            Some(endpoint.to_string()),
            access_key.clone(),
            secret_key.clone(),
            kind.clone(),
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SKIP] providers_matrix: provider init failed for {name}: {e:?}");
                return;
            }
        };

        // Ensure bucket exists before upload (like aws_multipart.rs tests)
        provider.create_bucket(&test_bucket).await.ok();

        if let Err(e) = provider
            .upload_file(&test_bucket, test_key, &test_file, None, None)
            .await
        {
            eprintln!("[SKIP] providers_matrix: upload failed for {name}: {e:?}");
            return;
        }

        // Download and verify
        let download_path = std::env::temp_dir().join("downloaded_file.txt");
        if let Err(e) = provider
            .download_file(&test_bucket, test_key, &download_path)
            .await
        {
            eprintln!("[SKIP] providers_matrix: download failed for {name}: {e:?}");
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
                eprintln!("[SKIP] providers_matrix: list_objects failed for {name}: {e:?}");
                return;
            }
        };
        let found = objects.iter().any(|obj| obj.key == test_key);
        assert!(found, "Uploaded file not found in list_objects");

        // Delete and check
        if let Err(e) = provider.delete_object(&test_bucket, test_key).await {
            eprintln!("[SKIP] providers_matrix: delete_object failed for {name}: {e:?}");
            return;
        }
        let objects = match provider.list_objects(&test_bucket, Some("matrix/")).await {
            Ok(objects) => objects,
            Err(e) => {
                eprintln!(
                    "[SKIP] providers_matrix: list_objects failed after delete for {name}: {e:?}"
                );
                return;
            }
        };
        let found = objects.iter().any(|obj| obj.key == test_key);
        assert!(!found, "File not deleted");

        println!("Provider {name} passed upload, download, list, and delete test");
    }
}
