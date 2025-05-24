use crate::{Bucket, Metadata, ObjectMetadata, StorageError, StorageObject, StorageProvider};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::{
    operation::{
        delete_object::DeleteObjectOutput, get_object::GetObjectOutput,
        head_object::HeadObjectOutput, list_buckets::ListBucketsOutput,
        list_objects_v2::ListObjectsV2Output, put_object::PutObjectOutput,
    },
    primitives::ByteStream,
    Client,
};
use aws_smithy_types::DateTime;
use chrono;

use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use log::{debug, error, info};

use std::path::Path;
use std::pin::Pin;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::{fs::File, io::AsyncReadExt};

/// AWS S3 storage provider
#[derive(Debug, Clone)]
pub enum ProviderKind {
    Aws,
    Minio,
    Cloudflare,
    Gcp,
    Localstack,
    Other(String),
}

pub struct S3Provider {
    /// S3 client
    client: Client,
    /// Region
    region: String,
    /// Custom endpoint
    endpoint: Option<String>,
    /// Provider kind (for provider-specific config/quirks)
    #[allow(dead_code)]
    provider_kind: ProviderKind,
}

impl S3Provider {
    /// Creates a new S3 provider
    pub async fn new_with_kind(
        region: Option<String>,
        endpoint: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
        provider_kind: ProviderKind,
    ) -> Result<Self, StorageError> {
        use log::info;
        let region_str = region.clone().unwrap_or_else(|| "us-east-1".to_string());
        info!("Initializing S3Provider for {:?}", provider_kind);
        let region = Region::new(region_str.clone());

        let mut config_builder = aws_config::defaults(BehaviorVersion::v2025_01_17())
            .region(region)
            .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(3));

        // Add credentials if provided
        if let (Some(access_key), Some(secret_key)) = (access_key.clone(), secret_key.clone()) {
            let credentials = aws_credential_types::Credentials::new(
                access_key, secret_key, None, None, "explicit",
            );
            config_builder =
                config_builder.credentials_provider(SharedCredentialsProvider::new(credentials));
        }

        // Provider-specific endpoint and config
        let mut force_path_style = false;
        let mut default_endpoint = endpoint.clone();
        match provider_kind {
            ProviderKind::Aws => {}
            ProviderKind::Minio | ProviderKind::Localstack => {
                force_path_style = true;
                if default_endpoint.is_none() {
                    default_endpoint = Some("http://localhost:9000".to_string());
                }
            }
            ProviderKind::Cloudflare => {
                // Cloudflare R2: force path style and custom endpoint
                force_path_style = true;
            }
            ProviderKind::Gcp => {
                // GCP Interop: may need path style
                force_path_style = true;
            }
            ProviderKind::Other(_) => {}
        }
        if let Some(ref ep) = default_endpoint {
            config_builder = config_builder.endpoint_url(ep.clone());
        }

        // Build the base AWS config
        let sdk_config = config_builder.load().await;
        let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config);
        if force_path_style {
            s3_config_builder = s3_config_builder.force_path_style(true);
        }
        let s3_config = s3_config_builder.build();
        let client = Client::from_conf(s3_config);
        Ok(Self {
            client,
            region: region_str,
            endpoint: default_endpoint,
            provider_kind,
        })
    }

    /// Creates a new S3 provider
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        region: Option<String>,
        endpoint: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
    ) -> Result<Self, StorageError> {
        let region_str = region.unwrap_or("us-east-1".to_string());
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
        if let Some(ref endpoint) = endpoint {
            info!("Using custom endpoint: {}", endpoint);
            config_builder = config_builder.endpoint_url(endpoint);
        } else {
            info!("Using default AWS endpoint for region: {}", region_str);
            // Explicitly construct the endpoint URL for the region
            let default_endpoint = format!("https://s3.{}.amazonaws.com", region_str);
            info!("Constructed default endpoint URL: {}", default_endpoint);
            config_builder = config_builder.endpoint_url(default_endpoint);
        }

        // Build the base AWS config
        let sdk_config = config_builder.load().await;

        // Build S3 config, enabling path-style if endpoint is set
        let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config);
        if endpoint.is_some() {
            s3_config_builder = s3_config_builder.force_path_style(true);
        }
        let s3_config = s3_config_builder.build();

        // Create the S3 client
        let client = Client::from_conf(s3_config);

        Ok(Self {
            client,
            region: region_str,
            endpoint,
            provider_kind: ProviderKind::Aws,
        })
    }

    /// Converts an S3 object to a StorageObject
    fn convert_s3_object(&self, obj: &aws_sdk_s3::types::Object) -> StorageObject {
        StorageObject {
            key: obj.key().unwrap_or_default().to_string(),
            size: obj.size().map(|size| size.try_into().unwrap_or(0)),
            last_modified: obj.last_modified().map(|t| {
                let secs = t.secs();
                let nanos = 0; // AWS DateTime doesn't provide nanoseconds directly
                chrono::DateTime::from_timestamp(secs, nanos).unwrap_or_else(chrono::Utc::now)
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
        metadata.cloned()
    }

    /// Helper: fetch object from S3 with error mapping
    pub async fn get_object_with_error_handling(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<GetObjectOutput, StorageError> {
        self.client
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
            })
    }

    /// Helper: create parent directories for a file path
    pub async fn create_parent_dirs(&self, file_path: &Path) -> Result<(), StorageError> {
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                error!("Failed to create directory {}: {}", parent.display(), e);
                StorageError::Io(e)
            })?;
        }
        Ok(())
    }

    /// Helper: stream S3 body to file, returns bytes written
    pub async fn stream_s3_to_file<R>(
        &self,
        mut stream: R,
        file_path: &Path,
    ) -> Result<u64, StorageError>
    where
        R: tokio::io::AsyncRead + Unpin + Send,
    {
        let mut file = File::create(file_path).await.map_err(|e| {
            error!("Failed to create file {}: {}", file_path.display(), e);
            StorageError::Io(e)
        })?;
        let mut bytes_written = 0u64;
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
            bytes_written += n as u64;
        }
        file.flush().await.map_err(|e| {
            error!("Failed to flush file: {}", e);
            StorageError::Io(e)
        })?;
        Ok(bytes_written)
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
#[async_trait]
impl StorageProvider for S3Provider {
    fn name(&self) -> &str {
        "AWS S3"
    }

    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError> {
        // First check if bucket exists
        match self.client.head_bucket().bucket(bucket).send().await {
            Ok(_) => {
                info!("Bucket {} already exists", bucket);
                return Ok(());
            }
            Err(e)
                if e.as_service_error()
                    .map(|e| e.is_not_found())
                    .unwrap_or(false) =>
            {
                let create_bucket_result = self
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
                return Ok(());
            }
            Err(e) => {
                error!("Error checking bucket existence: {}", e);
                return Err(StorageError::Aws(e.to_string()));
            }
        }
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
                creation_date: b.creation_date().map(to_system_time),
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
        use aws_sdk_s3::primitives::ByteStream;
        use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
        use tokio::io::{AsyncReadExt, BufReader};

        const PART_SIZE: usize = 5 * 1024 * 1024; // 5MB (S3 minimum)

        let file = tokio::fs::File::open(file_path).await.map_err(|e| {
            error!("Failed to open file {}: {}", file_path.display(), e);
            StorageError::Io(e)
        })?;
        let metadata_fs = file.metadata().await.map_err(|e| {
            error!("Failed to get file metadata: {}", e);
            StorageError::Io(e)
        })?;
        let file_size = metadata_fs.len();
        let mut reader = BufReader::new(file);

        // Use single put_object for small files
        if file_size <= PART_SIZE as u64 {
            let mut buffer = Vec::with_capacity(file_size as usize);
            reader.read_to_end(&mut buffer).await.map_err(|e| {
                error!("Failed to read file {}: {}", file_path.display(), e);
                StorageError::Io(e)
            })?;
            let mut put_object_request = self
                .client
                .put_object()
                .bucket(bucket)
                .key(key)
                .body(ByteStream::from(buffer));
            if let Some(content_type) = content_type {
                put_object_request = put_object_request.content_type(content_type);
            }
            if let Some(metadata) = metadata {
                for (key, value) in metadata {
                    put_object_request = put_object_request.metadata(key, value);
                }
            }
            put_object_request.send().await.map_err(|e| {
                error!("Failed to upload file to {}/{}: {}", bucket, key, e);
                StorageError::Aws(e.to_string())
            })?;
            info!(
                "Uploaded file {} to {}/{} ({} bytes) in single part",
                file_path.display(),
                bucket,
                key,
                file_size
            );
            return Ok(());
        }

        // Multipart upload for large files
        debug!(
            "Initiating multipart upload: bucket={}, key={}, file_size={}",
            bucket, key, file_size
        );
        let create_resp = self
            .client
            .create_multipart_upload()
            .bucket(bucket)
            .key(key)
            .set_content_type(content_type.map(|s| s.to_string()))
            .send()
            .await
            .map_err(|e| {
                error!("[DEBUG] Failed to initiate multipart upload: {}", e);
                StorageError::Aws(e.to_string())
            })?;
        let upload_id = create_resp
            .upload_id()
            .ok_or_else(|| {
                StorageError::Aws("No upload_id returned from create_multipart_upload".to_string())
            })?
            .to_string();
        let mut parts: Vec<CompletedPart> = Vec::new();
        let mut part_number = 1;
        loop {
            let mut buf = vec![0u8; PART_SIZE];
            let mut filled = 0;
            // Fill the buffer up to PART_SIZE or until EOF
            while filled < PART_SIZE {
                let n = reader.read(&mut buf[filled..]).await.map_err(|e| {
                    error!("[DEBUG] Failed to read file part: {}", e);
                    StorageError::Io(e)
                })?;
                if n == 0 {
                    break;
                }
                filled += n;
            }
            if filled == 0 {
                break;
            } // EOF
            let is_last_part = filled < PART_SIZE;
            // S3: All parts except the last must be at least PART_SIZE (5MB)
            if !is_last_part && filled < PART_SIZE {
                error!(
                    "Part {} is too small ({} bytes, must be >= {} except last). Aborting upload.",
                    part_number, filled, PART_SIZE
                );
                // Abort upload
                std::mem::drop(
                    self.client
                        .abort_multipart_upload()
                        .bucket(bucket)
                        .key(key)
                        .upload_id(&upload_id)
                        .send(),
                );
                return Err(StorageError::Aws(format!(
                    "Multipart upload failed: part {} too small ({} bytes)",
                    part_number, filled
                )));
            }
            println!("Uploading part {} ({} bytes)", part_number, filled);
            debug!("Uploading part {} ({} bytes)", part_number, filled);
            info!("Uploading part {} ({} bytes)", part_number, filled);
            debug!(
                "Part {} first 8 bytes: {:?}",
                part_number,
                &buf[..8.min(filled)]
            );
            let part_resp = self
                .client
                .upload_part()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .part_number(part_number)
                .body(ByteStream::from(buf[..filled].to_vec()))
                .send()
                .await
                .map_err(|e| {
                    error!("Failed to upload part {}: {}", part_number, e);
                    // Abort upload on error
                    error!("Aborting multipart upload: upload_id={}", upload_id);
                    std::mem::drop(
                        self.client
                            .abort_multipart_upload()
                            .bucket(bucket)
                            .key(key)
                            .upload_id(&upload_id)
                            .send(),
                    );
                    StorageError::Aws(e.to_string())
                })?;
            let etag_raw = part_resp.e_tag().unwrap_or_default();
            info!("Part {} ETag as returned: {:?}", part_number, etag_raw);
            // DO NOT strip quotes; send ETag exactly as returned by S3/MinIO
            let completed_part = CompletedPart::builder()
                .part_number(part_number)
                .set_e_tag(Some(etag_raw.to_string()))
                .build();
            info!(
                "Will send part {} ETag (verbatim): {:?}",
                part_number,
                completed_part.e_tag()
            );
            parts.push(completed_part);
            info!("Uploaded part {} ({} bytes)", part_number, filled);
            part_number += 1;
            if is_last_part {
                break;
            }
        }
        // Complete upload
        // Ensure parts are sorted by part_number (S3 requires this)
        parts.sort_by_key(|p| p.part_number());
        for part in &parts {
            debug!(
                "Completing part: part_number={:?}, e_tag={:?}",
                part.part_number(),
                part.e_tag()
            );
        }
        // Assert all parts except the last are at least 5MB
        // We can't get the size from CompletedPart, so log a warning instead
        for (i, part) in parts.iter().enumerate() {
            if i < parts.len() - 1 {
                info!("Part {}: part_number={:?}, e_tag={:?} (size unknown, must be >=5MB except last)", i+1, part.part_number(), part.e_tag());
            }
        }
        info!("CompletedMultipartUpload payload: parts={:?}", parts);
        let completed_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(parts))
            .build();
        let complete_result = self
            .client
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(completed_upload)
            .send()
            .await;
        match complete_result {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to complete multipart upload: {:#?}", e);
                return Err(StorageError::Aws(e.to_string()));
            }
        }
        info!(
            "Uploaded file {} to {}/{} ({} bytes) via multipart upload",
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
        let get_object_result = self.get_object_with_error_handling(bucket, key).await?;
        let content_length = get_object_result.content_length().unwrap_or(0) as u64;
        self.create_parent_dirs(file_path).await?;
        let bytes_written = self
            .stream_s3_to_file(get_object_result.body.into_async_read(), file_path)
            .await?;
        info!(
            "Downloaded object {}/{} to {} ({} bytes)",
            bucket,
            key,
            file_path.display(),
            bytes_written
        );
        if content_length != 0 && content_length != bytes_written {
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
            size: head_object_result
                .content_length()
                .map(|size| size.try_into().unwrap_or(0)),
            last_modified: head_object_result.last_modified().map(|t| {
                let secs = t.secs();
                let nanos = 0; // AWS DateTime doesn't provide nanoseconds directly
                chrono::DateTime::from_timestamp(secs, nanos).unwrap_or_else(chrono::Utc::now)
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
        expires_in: std::time::Duration,
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
