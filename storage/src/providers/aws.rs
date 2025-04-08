use crate::{Bucket, Metadata, ObjectMetadata, StorageError, StorageObject, StorageProvider};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::{
    operation::{
        create_bucket::CreateBucketOutput, delete_object::DeleteObjectOutput,
        get_object::GetObjectOutput, head_object::HeadObjectOutput,
        list_buckets::ListBucketsOutput, list_objects_v2::ListObjectsV2Output,
        put_object::PutObjectOutput,
    },
    primitives::ByteStream,
    Client,
};
use aws_smithy_types::DateTime;
use chrono;

use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use log::{error, info};
use std::convert::TryFrom;
use std::path::Path;
use std::pin::Pin;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::{fs::File, io::AsyncReadExt};

/// AWS S3 storage provider
pub struct S3Provider {
    /// S3 client
    client: Client,
    /// Region
    region: String,
    /// Custom endpoint
    endpoint: Option<String>,
}

impl S3Provider {
    /// Creates a new S3 provider
    pub async fn new(
        region: Option<String>,
        endpoint: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
    ) -> Result<Self, StorageError> {
        let region_str = region.unwrap_or_else(|| "us-east-1".to_string());
        let region = Region::new(region_str.clone());

        let mut config_builder = aws_config::defaults(BehaviorVersion::v2025_01_17())
            .region(region)
            .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(3));

        // Add credentials if provided
        if let (Some(access_key), Some(secret_key)) = (access_key.clone(), secret_key.clone()) {
            let credentials = aws_sdk_s3::config::Credentials::new(
                access_key,
                secret_key,
                None,
                None,
                "static-credentials-provider",
            );
            config_builder =
                config_builder.credentials_provider(SharedCredentialsProvider::new(credentials));
        }

        // Add custom endpoint if provided
        if let Some(endpoint) = endpoint.clone() {
            info!("Using custom endpoint: {}", endpoint);
            config_builder = config_builder.endpoint_url(endpoint);
        } else {
            info!("Using default AWS endpoint for region: {}", region_str);
            // Explicitly construct the endpoint URL for the region
            let default_endpoint = format!("https://s3.{}.amazonaws.com", region_str);
            info!("Constructed default endpoint URL: {}", default_endpoint);
            config_builder = config_builder.endpoint_url(default_endpoint);
        }

        // Build the config
        let sdk_config = config_builder.load().await;

        // Create the S3 client
        let client = Client::new(&sdk_config);

        Ok(Self {
            client,
            region: region_str,
            endpoint,
        })
    }

    /// Converts an S3 object to a StorageObject
    fn convert_s3_object(&self, obj: &aws_sdk_s3::types::Object) -> StorageObject {
        StorageObject {
            key: obj.key().unwrap_or_default().to_string(),
            size: match obj.size() {
                Some(size) => Some(size.try_into().unwrap_or(0)),
                None => None,
            },
            last_modified: obj.last_modified().map(|t| {
                let secs = t.secs();
                let nanos = 0; // AWS DateTime doesn't provide nanoseconds directly
                chrono::DateTime::from_timestamp(secs, nanos).unwrap_or_else(|| chrono::Utc::now())
            }),
            etag: obj.e_tag().map(|s| s.to_string()),
            storage_class: obj.storage_class().map(|s| s.as_str().to_string()),
        }
    }

    /// Converts S3 metadata to a Metadata map
    fn extract_metadata(
        &self,
        metadata: Option<&std::collections::HashMap<String, String>>,
    ) -> Option<Metadata> {
        metadata.map(|m| m.clone())
    }
}

fn to_system_time(dt: &DateTime) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(dt.secs() as u64)
}

// #[async_trait]
// impl StreamUploadProvider for S3Provider {
//     async fn upload_stream<S>(
//         &self,
//         bucket: &str,
//         key: &str,
//         stream: S,
//         content_type: Option<&str>,
//         metadata: Option<Metadata>,
//     ) -> Result<(), StorageError>
//     where
//         S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
//     {
//         // Convert the stream to a ByteStream that AWS SDK can use
//         let byte_stream = ByteStream::new(stream
//             .map(|result| result.map_err(|e| {
//                 error!("Stream error: {}", e);
//                 std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
//             }))
//             .map_ok(|bytes| bytes.to_vec()));

//         let mut put_object_request = self.client.put_object().bucket(bucket).key(key).body(byte_stream);

//         if let Some(content_type) = content_type {
//             put_object_request = put_object_request.content_type(content_type);
//         }

//         if let Some(metadata) = metadata {
//             for (key, value) in metadata {
//                 put_object_request = put_object_request.metadata(key, value);
//             }
//         }

//         let _put_object_result: PutObjectOutput = put_object_request.send().await.map_err(|e| {
//             error!("Failed to upload object {}/{}: {}", bucket, key, e);
//             StorageError::Aws(e.to_string())
//         })?;

//         info!("Uploaded object {}/{}", bucket, key);
//         Ok(())
//     }
// }

#[async_trait]
impl StorageProvider for S3Provider {
    fn name(&self) -> &str {
        "AWS S3"
    }

    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError> {
        let create_bucket_result: CreateBucketOutput = self
            .client
            .create_bucket()
            .bucket(bucket)
            .create_bucket_configuration(
                aws_sdk_s3::types::CreateBucketConfiguration::builder()
                    .location_constraint(aws_sdk_s3::types::BucketLocationConstraint::from(
                        self.region.as_str(),
                    ))
                    .build(),
            )
            .send()
            .await
            .map_err(|e| {
                error!("Failed to create bucket {}: {}", bucket, e);
                StorageError::Aws(e.to_string())
            })?;

        info!("Created bucket: {:?}", create_bucket_result.location());
        Ok(())
    }

    async fn bucket_exists(&self, bucket: &str) -> Result<bool, StorageError> {
        info!("Checking if bucket exists: {}", bucket);
        info!("Using region: {}", self.region);
        if let Some(endpoint) = &self.endpoint {
            info!("Using endpoint: {}", endpoint);
        }

        let head_bucket_request = self.client.head_bucket().bucket(bucket);
        info!("Sending head_bucket request to AWS S3");

        match head_bucket_request.send().await {
            Ok(_) => {
                info!("Bucket {} exists", bucket);
                Ok(true)
            }
            Err(e) => {
                let error_string = e.to_string();
                info!("Received error response: {}", error_string);

                if error_string.contains("404") {
                    info!("Bucket {} does not exist (404 Not Found)", bucket);
                    Ok(false)
                } else if error_string.contains("403") {
                    info!(
                        "Bucket {} exists but access is forbidden (403 Forbidden)",
                        bucket
                    );
                    // For S3, a 403 means the bucket exists but we don't have permission to access it
                    // We'll treat this as the bucket existing
                    Ok(true)
                } else {
                    let error_msg = format!("Error checking if bucket exists: {}", e);
                    error!("{}", error_msg);
                    Err(StorageError::AwsSdk(e.to_string()))
                }
            }
        }
    }

    async fn list_buckets(&self) -> Result<Vec<Bucket>, StorageError> {
        let list_buckets_result: ListBucketsOutput =
            self.client.list_buckets().send().await.map_err(|e| {
                error!("Failed to list buckets: {}", e);
                StorageError::Aws(e.to_string())
            })?;

        // Get the buckets or use an empty vec if None
        let buckets = list_buckets_result.buckets();

        let result = buckets
            .iter()
            .map(|b| Bucket {
                name: b.name().unwrap_or_default().to_string(),
                creation_date: b.creation_date().map(|t| to_system_time(t)),
                region: Some(self.region.clone()),
            })
            .collect();

        Ok(result)
    }

    async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<StorageObject>, StorageError> {
        let mut list_objects_request = self.client.list_objects_v2().bucket(bucket);

        if let Some(prefix) = prefix {
            list_objects_request = list_objects_request.prefix(prefix);
        }

        let list_objects_result: ListObjectsV2Output =
            list_objects_request.send().await.map_err(|e| {
                error!("Failed to list objects in bucket {}: {}", bucket, e);
                StorageError::Aws(e.to_string())
            })?;

        let objects = list_objects_result
            .contents()
            .iter()
            .map(|obj| self.convert_s3_object(obj))
            .collect();

        Ok(objects)
    }

    async fn upload_file(
        &self,
        bucket: &str,
        key: &str,
        file_path: &Path,
        content_type: Option<&str>,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError> {
        let file = tokio::fs::File::open(file_path).await.map_err(|e| {
            error!("Failed to open file {}: {}", file_path.display(), e);
            StorageError::Io(e)
        })?;

        let file_size = file
            .metadata()
            .await
            .map_err(|e| {
                error!("Failed to get file metadata: {}", e);
                StorageError::Io(e)
            })?
            .len();

        // Read the file into a buffer
        let mut buffer = Vec::new();
        let mut file = tokio::fs::File::open(file_path).await.map_err(|e| {
            error!("Failed to open file {}: {}", file_path.display(), e);
            StorageError::Io(e)
        })?;
        file.read_to_end(&mut buffer).await.map_err(|e| {
            error!("Failed to read file {}: {}", file_path.display(), e);
            StorageError::Io(e)
        })?;

        // Create a ByteStream from the buffer
        let stream = ByteStream::from(buffer);
        let mut put_object_request = self
            .client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(stream);

        if let Some(content_type) = content_type {
            put_object_request = put_object_request.content_type(content_type);
        }

        if let Some(metadata) = metadata {
            for (key, value) in metadata {
                put_object_request = put_object_request.metadata(key, value);
            }
        }

        let _put_object_result: PutObjectOutput = put_object_request.send().await.map_err(|e| {
            error!("Failed to upload file to {}/{}: {}", bucket, key, e);
            StorageError::Aws(e.to_string())
        })?;

        info!(
            "Uploaded file {} to {}/{} ({} bytes)",
            file_path.display(),
            bucket,
            key,
            file_size
        );
        Ok(())
    }

    async fn download_file(
        &self,
        bucket: &str,
        key: &str,
        file_path: &Path,
    ) -> Result<(), StorageError> {
        let get_object_result: GetObjectOutput = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to get object {}/{}: {}", bucket, key, e);
                if e.to_string().contains("404") {
                    StorageError::NotFound(format!("Object {}/{} not found", bucket, key))
                } else {
                    StorageError::Aws(e.to_string())
                }
            })?;

        let content_length = match get_object_result.content_length() {
            Some(size) => size.try_into().unwrap_or(0),
            None => 0,
        };

        // Create parent directories if they don't exist
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                error!("Failed to create directory {}: {}", parent.display(), e);
                StorageError::Io(e)
            })?;
        }

        let mut file = File::create(file_path).await.map_err(|e| {
            error!("Failed to create file {}: {}", file_path.display(), e);
            StorageError::Io(e)
        })?;

        let mut stream = get_object_result.body.into_async_read();
        let mut bytes_written = 0;

        let mut buffer = vec![0u8; 8192]; // 8KB buffer
        loop {
            let n = stream.read(&mut buffer).await.map_err(|e| {
                error!("Failed to read from stream: {}", e);
                StorageError::Io(e)
            })?;

            if n == 0 {
                break;
            }

            file.write_all(&buffer[0..n]).await.map_err(|e| {
                error!("Failed to write to file: {}", e);
                StorageError::Io(e)
            })?;

            bytes_written += u64::try_from(n).unwrap_or(0);
        }

        file.flush().await.map_err(|e| {
            error!("Failed to flush file: {}", e);
            StorageError::Io(e)
        })?;

        info!(
            "Downloaded object {}/{} to {} ({} bytes)",
            bucket,
            key,
            file_path.display(),
            bytes_written
        );

        // Verify the download size
        if content_length != bytes_written {
            error!(
                "Download size mismatch: expected {} bytes, got {} bytes",
                content_length, bytes_written
            );
            return Err(StorageError::Unexpected(format!(
                "Download size mismatch: expected {} bytes, got {} bytes",
                content_length, bytes_written
            )));
        }

        Ok(())
    }

    async fn download_stream(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>, StorageError>
    {
        let get_object_result: GetObjectOutput = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to get object {}/{}: {}", bucket, key, e);
                if e.to_string().contains("404") {
                    StorageError::NotFound(format!("Object {}/{} not found", bucket, key))
                } else {
                    StorageError::Aws(e.to_string())
                }
            })?;

        // Convert ByteStream to our expected return type
        let byte_stream = get_object_result.body;

        // Create a stream that collects all bytes and then yields them
        let collected_stream = byte_stream.collect().await.map_err(|e| {
            error!("Failed to collect stream: {}", e);
            StorageError::Aws(e.to_string())
        })?;

        // Create a once stream that yields the collected bytes
        let once_stream = futures::stream::once(futures::future::ok::<Bytes, std::io::Error>(
            collected_stream.into_bytes(),
        ));

        Ok(Box::pin(once_stream))
    }

    async fn get_object_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMetadata, StorageError> {
        let head_object_result: HeadObjectOutput = self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to get object metadata {}/{}: {}", bucket, key, e);
                if e.to_string().contains("404") {
                    StorageError::NotFound(format!("Object {}/{} not found", bucket, key))
                } else {
                    StorageError::Aws(e.to_string())
                }
            })?;

        let metadata = ObjectMetadata {
            key: key.to_string(),
            size: match head_object_result.content_length() {
                Some(size) => Some(size.try_into().unwrap_or(0)),
                None => None,
            },
            last_modified: head_object_result.last_modified().map(|t| {
                let secs = t.secs();
                let nanos = 0; // AWS DateTime doesn't provide nanoseconds directly
                chrono::DateTime::from_timestamp(secs, nanos).unwrap_or_else(|| chrono::Utc::now())
            }),
            etag: head_object_result.e_tag().map(|s| s.to_string()),
            content_type: head_object_result.content_type().map(|s| s.to_string()),
            storage_class: head_object_result
                .storage_class()
                .map(|s| s.as_str().to_string()),
            metadata: self.extract_metadata(head_object_result.metadata()),
        };

        Ok(metadata)
    }

    async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        let _delete_object_result: DeleteObjectOutput = self
            .client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to delete object {}/{}: {}", bucket, key, e);
                StorageError::Aws(e.to_string())
            })?;

        info!("Deleted object {}/{}", bucket, key);
        Ok(())
    }

    async fn object_exists(&self, bucket: &str, key: &str) -> Result<bool, StorageError> {
        match self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.to_string().contains("404") {
                    Ok(false)
                } else {
                    error!("Error checking if object exists: {}", e);
                    Err(StorageError::AwsSdk(e.to_string()))
                }
            }
        }
    }

    async fn generate_presigned_url(
        &self,
        bucket: &str,
        key: &str,
        expires_in: Duration,
    ) -> Result<String, StorageError> {
        let presigner = aws_sdk_s3::presigning::PresigningConfig::builder()
            .expires_in(expires_in)
            .build()
            .map_err(|e| {
                error!("Failed to build presigning config: {}", e);
                StorageError::Configuration(e.to_string())
            })?;

        let presigned_request = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(presigner)
            .await
            .map_err(|e| {
                error!("Failed to generate presigned URL: {}", e);
                StorageError::Aws(e.to_string())
            })?;

        Ok(presigned_request.uri().to_string())
    }

    async fn upload_stream(
        &self,
        bucket: &str,
        key: &str,
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        content_type: Option<&str>,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError> {
        // Collect all bytes from the stream into a single buffer
        let mut buffer = Vec::new();
        let mut stream = stream;

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    buffer.extend_from_slice(&chunk);
                }
                Err(e) => {
                    error!("Stream error: {}", e);
                    return Err(StorageError::Io(e));
                }
            }
        }

        // Create a ByteStream from the collected buffer
        let byte_stream = ByteStream::from(buffer);

        let mut put_object_request = self
            .client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(byte_stream);

        if let Some(content_type) = content_type {
            put_object_request = put_object_request.content_type(content_type);
        }

        if let Some(metadata) = metadata {
            for (key, value) in metadata {
                put_object_request = put_object_request.metadata(key, value);
            }
        }

        let _put_object_result: PutObjectOutput = put_object_request.send().await.map_err(|e| {
            error!("Failed to upload object {}/{}: {}", bucket, key, e);
            StorageError::Aws(e.to_string())
        })?;

        info!("Uploaded object {}/{}", bucket, key);
        Ok(())
    }
}
