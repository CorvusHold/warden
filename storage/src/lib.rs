//! Standardized storage library for S3-compatible storage providers.
//!
//! This library provides a unified interface for interacting with various
//! S3-compatible storage backends, including AWS S3, Cloudflare R2, and Google Cloud Storage.
//! It supports streaming uploads and downloads to optimize large backup operations.

mod error;
mod integration;
pub mod providers;
mod types;

pub use error::StorageError;
pub use integration::PostgresBackupStorage;
pub use providers::*;
pub use types::*;

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use std::path::Path;
use std::pin::Pin;

// /// Stream upload extension trait for storage providers.
// /// This trait is separate from StorageProvider to maintain object safety.
// #[async_trait]
// pub trait StreamUploadProvider: Send + Sync + 'static {
//     /// Uploads data from a stream to the storage provider.
//     async fn upload_stream<S>(
//         &self,
//         bucket: &str,
//         key: &str,
//         stream: S,
//         content_type: Option<&str>,
//         metadata: Option<Metadata>,
//     ) -> Result<(), StorageError>
//     where
//         S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static;
// }

/// Core storage provider interface for S3-compatible storage services.
#[async_trait]
pub trait StorageProvider: Send + Sync + 'static {
    /// Returns the name of the storage provider.
    fn name(&self) -> &str;

    /// Creates a new bucket if it doesn't exist.
    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError>;

    /// Checks if a bucket exists.
    async fn bucket_exists(&self, bucket: &str) -> Result<bool, StorageError>;

    /// Lists all buckets.
    async fn list_buckets(&self) -> Result<Vec<Bucket>, StorageError>;

    /// Lists objects in a bucket with an optional prefix.
    async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<StorageObject>, StorageError>;

    /// Uploads a file to the storage provider.
    async fn upload_file(
        &self,
        bucket: &str,
        key: &str,
        file_path: &Path,
        content_type: Option<&str>,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError>;

    /// Downloads an object to a file.
    async fn download_file(
        &self,
        bucket: &str,
        key: &str,
        file_path: &Path,
    ) -> Result<(), StorageError>;

    /// Downloads an object as a stream of bytes.
    async fn download_stream(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>, StorageError>;

    /// Gets metadata for an object.
    async fn get_object_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMetadata, StorageError>;

    /// Deletes an object.
    async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), StorageError>;

    /// Checks if an object exists.
    async fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError>;

    /// Generates a pre-signed URL for an object.
    async fn generate_presigned_url(
        &self,
        bucket: &str,
        key: &str,
        expires_in: std::time::Duration,
    ) -> Result<String, StorageError>;

    async fn upload_stream(
        &self,
        bucket: &str,
        key: &str,
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        content_type: Option<&str>,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError>;
}

/// Factory for creating storage providers.
pub struct StorageProviderFactory;

// Update the factory method
impl StorageProviderFactory {
    pub async fn create_s3_provider(
        region: Option<String>,
        endpoint: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
    ) -> Result<Box<dyn StorageProvider>, StorageError> {
        let provider =
            providers::aws::S3Provider::new(region, endpoint, access_key, secret_key).await?;
        Ok(Box::new(provider))
    }
}
