use rand::SeedableRng;
use std::fs::File;
use std::io::Write;
use storage::providers::aws::S3Provider;
use storage::StorageProvider;
use tempfile::tempdir;

#[tokio::test]
async fn test_multipart_upload_large_file() {
    let bucket = std::env::var("AWS_TEST_BUCKET").expect("Set AWS_TEST_BUCKET env var");
    let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
    let region = std::env::var("AWS_REGION").ok();
    let endpoint = std::env::var("AWS_ENDPOINT").ok();

    let provider = S3Provider::new(region, endpoint, access_key, secret_key)
        .await
        .expect("Failed to create S3Provider");

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("large_test_file.bin");
    let size = 20 * 1024 * 1024; // 20MB
    println!("[DEBUG] Test file size: {} bytes", size);
    use rand::RngCore;
    let mut file = File::create(&file_path).unwrap();
    let mut buf = vec![0u8; size];
    rand::rngs::StdRng::fill_bytes(&mut rand::rngs::StdRng::seed_from_u64(0), &mut buf);
    file.write_all(&buf).unwrap();

    let key = format!("test/large_file_{}.bin", uuid::Uuid::new_v4());
    // Ensure bucket exists before upload
    provider.create_bucket(&bucket).await.ok();
    provider
        .upload_file(
            &bucket,
            &key,
            &file_path,
            Some("application/octet-stream"),
            None,
        )
        .await
        .expect("Multipart upload failed");

    // Optionally, check that the object exists and size matches
    let meta = provider
        .get_object_metadata(&bucket, &key)
        .await
        .expect("meta");
    assert_eq!(meta.size, Some(size as u64));

    // Clean up
    provider.delete_object(&bucket, &key).await.expect("delete");
}

#[tokio::test]
async fn test_multipart_upload_non_multiple_of_5mb() {
    let bucket = std::env::var("AWS_TEST_BUCKET").expect("Set AWS_TEST_BUCKET env var");
    let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
    let region = std::env::var("AWS_REGION").ok();
    let endpoint = std::env::var("AWS_ENDPOINT").ok();

    let provider = S3Provider::new(region, endpoint, access_key, secret_key)
        .await
        .expect("Failed to create S3Provider");

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("non_multiple_5mb_file.bin");
    let size = 13 * 1024 * 1024; // 13MB (2*5MB + 3MB)
    println!("[DEBUG] Test file size: {} bytes", size);
    use rand::RngCore;
    let mut file = File::create(&file_path).unwrap();
    let mut buf = vec![0u8; size];
    rand::rngs::StdRng::fill_bytes(&mut rand::rngs::StdRng::seed_from_u64(42), &mut buf);
    file.write_all(&buf).unwrap();

    let key = format!("test/non_multiple_5mb_file_{}.bin", uuid::Uuid::new_v4());
    // Ensure bucket exists before upload
    provider.create_bucket(&bucket).await.ok();
    provider
        .upload_file(
            &bucket,
            &key,
            &file_path,
            Some("application/octet-stream"),
            None,
        )
        .await
        .expect("Multipart upload failed (non-multiple 5MB)");

    // Optionally, check that the object exists and size matches
    let meta = provider
        .get_object_metadata(&bucket, &key)
        .await
        .expect("meta");
    assert_eq!(meta.size, Some(size as u64));

    // Clean up
    provider.delete_object(&bucket, &key).await.expect("delete");
}

#[tokio::test]
async fn test_single_part_upload_small_file() {
    let bucket = std::env::var("AWS_TEST_BUCKET").expect("Set AWS_TEST_BUCKET env var");
    let access_key = std::env::var("AWS_ACCESS_KEY_ID").ok();
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY").ok();
    let region = std::env::var("AWS_REGION").ok();
    let endpoint = std::env::var("AWS_ENDPOINT").ok();

    let provider = S3Provider::new(region, endpoint, access_key, secret_key)
        .await
        .expect("Failed to create S3Provider");

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("small_test_file.bin");
    let size = 1024 * 1024; // 1MB
    println!("[DEBUG] Test file size: {} bytes", size);
    let mut file = File::create(&file_path).unwrap();
    file.write_all(&vec![21u8; size]).unwrap();

    let key = format!("test/small_file_{}.bin", uuid::Uuid::new_v4());
    // Ensure bucket exists before upload
    provider.create_bucket(&bucket).await.ok();
    provider
        .upload_file(
            &bucket,
            &key,
            &file_path,
            Some("application/octet-stream"),
            None,
        )
        .await
        .expect("Single part upload failed");

    let meta = provider
        .get_object_metadata(&bucket, &key)
        .await
        .expect("meta");
    assert_eq!(meta.size, Some(size as u64));
    provider.delete_object(&bucket, &key).await.expect("delete");
}
