use crate::{Bucket, Metadata, ObjectMetadata, StorageError, StorageObject, StorageProvider};
use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::{
    operation::{
        get_object::GetObjectOutput, list_buckets::ListBucketsOutput,
        list_objects_v2::ListObjectsV2Output,
    },
    types::CompletedMultipartUpload,
    Client,
};
use aws_smithy_types::DateTime;
use bytes::Bytes;
use chrono::{TimeZone, Utc};
use futures::Stream;
use log::{debug, error, info};
use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::{fs::File, io::AsyncReadExt};

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
    /// Helper: initiate a multipart upload and return the upload_id
    async fn initiate_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        content_type: Option<&str>,
        metadata: Option<Metadata>,
    ) -> Result<String, StorageError> {
        let mut req = self
            .client
            .create_multipart_upload()
            .bucket(bucket)
            .key(key);
        if let Some(content_type) = content_type {
            req = req.content_type(content_type);
        }
        if let Some(metadata) = metadata {
            for (k, v) in metadata {
                req = req.metadata(k, v);
            }
        }
        let resp = req.send().await.map_err(|e| {
            error!(
                "Failed to initiate multipart upload for {bucket}/{key}: {e}"
            );
            StorageError::Aws(e.to_string())
        })?;
        resp.upload_id()
            .map(|s| s.to_string())
            .ok_or_else(|| StorageError::Unexpected("No upload_id returned from S3".to_string()))
    }

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
        info!("Initializing S3Provider for {provider_kind:?}");
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
            info!("Using custom endpoint: {endpoint}");
            config_builder = config_builder.endpoint_url(endpoint);
        } else {
            info!("Using default AWS endpoint for region: {region_str}");
            // Explicitly construct the endpoint URL for the region
            let default_endpoint = format!("https://s3.{region_str}.amazonaws.com");
            info!("Constructed default endpoint URL: {default_endpoint}");
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
                Utc.timestamp_opt(secs, 0).single().unwrap_or_else(Utc::now)
            }),
            etag: obj.e_tag().map(|s| s.to_string()),
            storage_class: obj.storage_class().map(|s| s.as_str().to_string()),
        }
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
                error!("Failed to get object {bucket}/{key}: {e}");
                if e.to_string().contains("404") {
                    StorageError::NotFound(format!("Object {bucket}/{key} not found"))
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
                error!("Failed to read from stream: {e}");
                StorageError::Io(e)
            })?;
            if n == 0 {
                break;
            }
            file.write_all(&buffer[0..n]).await.map_err(|e| {
                error!("Failed to write to file: {e}");
                StorageError::Io(e)
            })?;
            bytes_written += n as u64;
        }
        file.flush().await.map_err(|e| {
            error!("Failed to flush file: {e}");
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
#[async_trait]
impl StorageProvider for S3Provider {
    fn name(&self) -> &str {
        "AWS S3"
    }

    // --- Required trait stubs ---
    async fn download_file(
        &self,
        bucket: &str,
        key: &str,
        destination: &Path,
    ) -> Result<(), StorageError> {
        use tokio::fs::File;
        use tokio::io::AsyncWriteExt;

        let resp = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    StorageError::NotFound(format!("Object {bucket}/{key} not found"))
                } else {
                    StorageError::Aws(msg)
                }
            })?;

        // Ensure parent directory exists
        self.create_parent_dirs(destination).await?;
        let mut file = File::create(destination).await.map_err(StorageError::Io)?;
        let mut stream = resp.body.into_async_read();
        tokio::io::copy(&mut stream, &mut file)
            .await
            .map_err(StorageError::Io)?;
        file.flush().await.map_err(StorageError::Io)?;
        Ok(())
    }

    async fn download_stream(
        &self,
        _bucket: &str,
        _key: &str,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send>>,
        StorageError,
    > {
        Err(StorageError::Unexpected(
            "download_stream not implemented".to_string(),
        ))
    }

    async fn get_object_metadata(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<ObjectMetadata, StorageError> {
        let resp = self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    StorageError::NotFound(format!("Object {bucket}/{key} not found"))
                } else {
                    StorageError::Aws(msg)
                }
            })?;

        let size = resp.content_length().map(|s| s as u64);
        let last_modified = resp.last_modified().and_then(|dt| {
            // aws_sdk_s3::primitives::DateTime -> ChronoDateTime<Utc>
            let ts = dt.secs();
            Utc.timestamp_opt(ts, 0).single()
        });
        let etag = resp.e_tag().map(|s| s.to_string());
        let content_type = resp.content_type().map(|s| s.to_string());
        let storage_class = resp.storage_class().map(|s| format!("{s:?}"));
        let metadata = if let Some(meta) = resp.metadata() {
            if !meta.is_empty() {
                Some(meta.clone())
            } else {
                None
            }
        } else {
            None
        };

        Ok(ObjectMetadata {
            key: key.to_string(),
            size,
            last_modified,
            etag,
            content_type,
            storage_class,
            metadata,
        })
    }

    async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), StorageError> {
        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    StorageError::NotFound(format!("Object {bucket}/{key} not found"))
                } else {
                    StorageError::Aws(msg)
                }
            })?;
        Ok(())
    }

    async fn object_exists(&self, _bucket: &str, _key: &str) -> Result<bool, StorageError> {
        Err(StorageError::Unexpected(
            "object_exists not implemented".to_string(),
        ))
    }

    async fn generate_presigned_url(
        &self,
        _bucket: &str,
        _key: &str,
        _expires_in: std::time::Duration,
    ) -> Result<String, StorageError> {
        Err(StorageError::Unexpected(
            "generate_presigned_url not implemented".to_string(),
        ))
    }

    async fn create_bucket(&self, bucket: &str) -> Result<(), StorageError> {
        // First check if bucket exists
        match self.client.head_bucket().bucket(bucket).send().await {
            Ok(_) => {
                info!("Bucket {bucket} already exists");
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
                        error!("Failed to create bucket {bucket}: {e}");
                        StorageError::Aws(e.to_string())
                    })?;

                info!("Created bucket: {:?}", create_bucket_result.location());
                return Ok(());
            }
            Err(e) => {
                error!("Error checking bucket existence: {e}");
                return Err(StorageError::Aws(e.to_string()));
            }
        }
    }

    async fn bucket_exists(&self, bucket: &str) -> Result<bool, StorageError> {
        info!("Checking if bucket exists: {}", bucket);
        info!("Using region: {}", self.region);
        if let Some(endpoint) = &self.endpoint {
            info!("Using endpoint: {endpoint}");
        }

        let head_bucket_request = self.client.head_bucket().bucket(bucket);
        info!("Sending head_bucket request to AWS S3");

        match head_bucket_request.send().await {
            Ok(_) => {
                info!("Bucket {bucket} exists");
                Ok(true)
            }
            Err(e) => {
                let error_string = e.to_string();
                info!("Received error response: {error_string}");

                if error_string.contains("404") {
                    info!("Bucket {bucket} does not exist (404 Not Found)");
                    Ok(false)
                } else if error_string.contains("403") {
                    info!(
                        "Bucket {bucket} exists but access is forbidden (403 Forbidden)"
                    );
                    // For S3, a 403 means the bucket exists but we don't have permission to access it
                    // We'll treat this as the bucket existing
                    Ok(true)
                } else {
                    let error_msg = format!("Error checking if bucket exists: {e}");
                    error!("{error_msg}");
                    Err(StorageError::AwsSdk(e.to_string()))
                }
            }
        }
    }

    async fn list_buckets(&self) -> Result<Vec<Bucket>, StorageError> {
        let list_buckets_result: ListBucketsOutput =
            self.client.list_buckets().send().await.map_err(|e| {
                error!("Failed to list buckets: {e}");
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
                error!("Failed to list objects in bucket {bucket}: {e}");
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
            error!("Failed to get file metadata: {e}");
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
                error!("Failed to upload file to {bucket}/{key}: {e}");
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
        let upload_id = self
            .initiate_multipart_upload(bucket, key, content_type, metadata)
            .await?;
        let mut parts: Vec<CompletedPart> = Vec::new();
        let mut part_number = 1;
        loop {
            let mut buf = vec![0u8; PART_SIZE];
            let mut filled = 0;
            // Fill the buffer up to PART_SIZE or until EOF
            while filled < PART_SIZE {
                let n = reader.read(&mut buf[filled..]).await.map_err(|e| {
                    error!("[DEBUG] Failed to read file part: {e}");
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
                    "Part {part_number} is too small ({filled} bytes, must be >= {PART_SIZE} except last). Aborting upload."
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
                    "Multipart upload failed: part {part_number} too small ({filled} bytes)"
                )));
            }
            info!("Uploading part {part_number} ({filled} bytes)");
            // Upload part
            let upload_part_resp = self
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
                    error!(
                        "Failed to upload part {part_number} for {bucket}/{key}: {e:?}"
                    );
                    debug!("S3 error debug: {e:?}");
                    report_s3_error_to_sentry(
                        "upload_file:upload_part",
                        &e as &dyn std::error::Error,
                        bucket,
                        key,
                        None,
                    );
                    StorageError::Aws(e.to_string())
                })?;
            parts.push(
                CompletedPart::builder()
                    .set_part_number(Some(part_number))
                    .set_e_tag(upload_part_resp.e_tag().map(|s| s.to_string()))
                    .build(),
            );
            part_number += 1;
            if is_last_part {
                break;
            }
        }
        // Complete multipart upload
        let completed_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(parts))
            .build();
        self.client
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(completed_upload)
            .send()
            .await
            .map_err(|e| {
                error!(
                    "Failed to complete multipart upload for {bucket}/{key}: {e:?}"
                );
                debug!("S3 error debug: {e:?}");
                report_s3_error_to_sentry(
                    "upload_file:complete_multipart_upload",
                    &e as &dyn std::error::Error,
                    bucket,
                    key,
                    None,
                );
                StorageError::Aws(e.to_string())
            })?;
        info!("Multipart upload completed: {}/{}", bucket, key);
        Ok(())
    }

    async fn upload_stream(
        &self,
        bucket: &str,
        key: &str,
        stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
        content_type: Option<&str>,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError> {
        use aws_sdk_s3::primitives::ByteStream;
        use futures::StreamExt;
        const PART_SIZE: usize = 5 * 1024 * 1024; // 5 MB
        let mut parts = Vec::new();
        let mut part_number = 1;
        let mut buffer = Vec::with_capacity(PART_SIZE);
        let mut s = stream;
        let upload_id = self
            .initiate_multipart_upload(bucket, key, content_type, metadata)
            .await?;
        while let Some(chunk_result) = s.next().await {
            let chunk = chunk_result.map_err(|e| {
                error!("Failed to read stream chunk: {e}");
                StorageError::Io(e)
            })?;
            buffer.extend_from_slice(&chunk);
            while buffer.len() >= PART_SIZE {
                let part = buffer.drain(..PART_SIZE).collect::<Vec<u8>>();
                let upload_part_resp = self
                    .client
                    .upload_part()
                    .bucket(bucket)
                    .key(key)
                    .upload_id(&upload_id)
                    .part_number(part_number)
                    .body(ByteStream::from(part))
                    .send()
                    .await
                    .map_err(|e| {
                        error!(
                            "Failed to upload part {part_number} for {bucket}/{key}: {e:?}"
                        );
                        StorageError::Aws(e.to_string())
                    })?;
                parts.push(
                    aws_sdk_s3::types::CompletedPart::builder()
                        .set_part_number(Some(part_number))
                        .set_e_tag(upload_part_resp.e_tag().map(|s| s.to_string()))
                        .build(),
                );
                part_number += 1;
            }
        }
        if !buffer.is_empty() {
            let upload_part_resp = self
                .client
                .upload_part()
                .bucket(bucket)
                .key(key)
                .upload_id(&upload_id)
                .part_number(part_number)
                .body(ByteStream::from(buffer))
                .send()
                .await
                .map_err(|e| {
                    error!(
                        "Failed to upload last part {part_number} for {bucket}/{key}: {e:?}"
                    );
                    StorageError::Aws(e.to_string())
                })?;
            parts.push(
                aws_sdk_s3::types::CompletedPart::builder()
                    .set_part_number(Some(part_number))
                    .set_e_tag(upload_part_resp.e_tag().map(|s| s.to_string()))
                    .build(),
            );
        }
        let completed_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(parts))
            .build();
        self.client
            .complete_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(completed_upload)
            .send()
            .await
            .map_err(|e| {
                error!(
                    "Failed to complete multipart upload for {bucket}/{key}: {e:?}"
                );
                StorageError::Aws(e.to_string())
            })?;
        info!("Multipart upload completed: {}/{}", bucket, key);
        Ok(())
    }
}

fn report_s3_error_to_sentry(
    operation: &str,
    error: &dyn std::error::Error,
    bucket: &str,
    key: &str,
    backup_id: Option<&str>,
) {
    let mut extra = HashMap::new();
    extra.insert("bucket", bucket);
    extra.insert("key", key);
    if let Some(backup_id) = backup_id {
        extra.insert("backup_id", backup_id);
    }

    let error_message = format!("{operation}: {error}");
    let extra_json = serde_json::to_string(&extra).unwrap_or_default();
    let sentry_message = format!("{error_message} | context: {extra_json}");
    sentry::capture_message(&sentry_message, sentry::Level::Error);
}
