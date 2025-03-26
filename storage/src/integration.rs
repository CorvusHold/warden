use crate::{Metadata, StorageError, StorageProvider, StorageProviderFactory, StorageProviderType};
use log::{error, info};
use std::path::{Path};
use std::time::Duration;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

/// Integration with PostgreSQL backup system
pub struct PostgresBackupStorage {
    /// Storage provider
    provider: Box<dyn StorageProvider>,
    /// Bucket name
    bucket: String,
    /// Base prefix for backups
    prefix: String,
}

impl PostgresBackupStorage {
    /// Creates a new PostgreSQL backup storage
    pub async fn new(
        provider_type: StorageProviderType,
        bucket: String,
        prefix: Option<String>,
        region: Option<String>,
        endpoint: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
        _account_id: Option<String>,
        _project_id: Option<String>,
        _credentials_path: Option<String>,
    ) -> Result<Self, StorageError> {
        // Create the appropriate storage provider
        let provider = match provider_type {
            StorageProviderType::S3 => {
                StorageProviderFactory::create_s3_provider(region, endpoint, access_key, secret_key).await?
            }
        };

        // Ensure the bucket exists
        if !provider.bucket_exists(&bucket).await? {
            info!("Bucket {} does not exist, creating it", bucket);
            provider.create_bucket(&bucket).await?;
        }

        Ok(Self {
            provider,
            bucket,
            prefix: prefix.unwrap_or_default(),
        })
    }

    /// Uploads a backup directory to storage
    pub async fn upload_backup(
        &self,
        backup_id: &str,
        backup_path: &Path,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError> {
        info!("Uploading backup {} from {}", backup_id, backup_path.display());

        // Create the backup prefix
        let backup_prefix = if self.prefix.is_empty() {
            backup_id.to_string()
        } else {
            format!("{}/{}", self.prefix, backup_id)
        };

        // Walk through the backup directory and upload all files
        let walker = walkdir::WalkDir::new(backup_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok());

        for entry in walker {
            if entry.file_type().is_file() {
                let rel_path = entry
                    .path()
                    .strip_prefix(backup_path)
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;

                let key = format!("{}/{}", backup_prefix, rel_path.to_string_lossy());
                
                // Determine content type based on file extension
                let content_type = match rel_path.extension().and_then(|e| e.to_str()) {
                    Some("sql") => Some("text/plain"),
                    Some("dump") => Some("application/octet-stream"),
                    Some("tar") => Some("application/x-tar"),
                    Some("gz") => Some("application/gzip"),
                    _ => None,
                };

                self.provider
                    .upload_file(&self.bucket, &key, entry.path(), content_type, metadata.clone())
                    .await?;
            }
        }

        info!("Backup {} uploaded successfully", backup_id);
        Ok(())
    }

    /// Uploads a backup file as a stream
    pub async fn upload_backup_stream(
        &self,
        backup_id: &str,
        file_name: &str,
        file_path: &Path,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError> {
        info!(
            "Streaming upload of backup file {} for backup {}",
            file_name, backup_id
        );

        // Create the backup key
        let key = if self.prefix.is_empty() {
            format!("{}/{}", backup_id, file_name)
        } else {
            format!("{}/{}/{}", self.prefix, backup_id, file_name)
        };

        // Determine content type based on file extension
        let content_type = match Path::new(file_name).extension().and_then(|e| e.to_str()) {
            Some("sql") => Some("text/plain"),
            Some("dump") => Some("application/octet-stream"),
            Some("tar") => Some("application/x-tar"),
            Some("gz") => Some("application/gzip"),
            _ => None,
        };

        // Open the file and create a stream
        let file = File::open(file_path).await.map_err(|e| {
            error!("Failed to open file {}: {}", file_path.display(), e);
            StorageError::Io(e)
        })?;

        let stream = ReaderStream::new(file);

        // Upload the stream
        self.provider
            .upload_stream(&self.bucket, &key, Box::pin(stream), content_type, metadata)
            .await?;

        info!("Backup file {} streamed successfully", file_name);
        Ok(())
    }

    /// Downloads a backup to a local directory
    pub async fn download_backup(
        &self,
        backup_id: &str,
        target_dir: &Path,
    ) -> Result<(), StorageError> {
        info!(
            "Downloading backup {} to {}",
            backup_id,
            target_dir.display()
        );

        // Create the backup prefix
        let backup_prefix = if self.prefix.is_empty() {
            backup_id.to_string()
        } else {
            format!("{}/{}", self.prefix, backup_id)
        };

        // List all objects with the backup prefix
        let objects = self
            .provider
            .list_objects(&self.bucket, Some(&backup_prefix))
            .await?;

        if objects.is_empty() {
            return Err(StorageError::NotFound(format!(
                "No objects found for backup {}",
                backup_id
            )));
        }

        // Create the target directory if it doesn't exist
        tokio::fs::create_dir_all(target_dir)
            .await
            .map_err(|e| StorageError::Io(e))?;

        // Download each object
        for obj in objects {
            let rel_path = obj
                .key
                .strip_prefix(&backup_prefix)
                .unwrap_or(&obj.key)
                .trim_start_matches('/');

            let target_path = target_dir.join(rel_path);

            // Create parent directories if they don't exist
            if let Some(parent) = target_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| StorageError::Io(e))?;
            }

            self.provider
                .download_file(&self.bucket, &obj.key, &target_path)
                .await?;
        }

        info!("Backup {} downloaded successfully", backup_id);
        Ok(())
    }

    /// Downloads a specific backup file
    pub async fn download_backup_file(
        &self,
        backup_id: &str,
        file_name: &str,
        target_path: &Path,
    ) -> Result<(), StorageError> {
        info!(
            "Downloading backup file {} from backup {} to {}",
            file_name,
            backup_id,
            target_path.display()
        );

        // Create the backup key
        let key = if self.prefix.is_empty() {
            format!("{}/{}", backup_id, file_name)
        } else {
            format!("{}/{}/{}", self.prefix, backup_id, file_name)
        };

        // Create parent directories if they don't exist
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::Io(e))?;
        }

        // Download the file
        self.provider
            .download_file(&self.bucket, &key, target_path)
            .await?;

        info!("Backup file {} downloaded successfully", file_name);
        Ok(())
    }

    /// Lists all backups
    pub async fn list_backups(&self) -> Result<Vec<String>, StorageError> {
        let prefix = if self.prefix.is_empty() {
            None
        } else {
            Some(self.prefix.as_str())
        };

        let objects = self.provider.list_objects(&self.bucket, prefix).await?;

        // Extract unique backup IDs from object keys
        let mut backup_ids = std::collections::HashSet::new();

        for obj in objects {
            let key = obj.key;
            let parts: Vec<&str> = key.split('/').collect();

            if parts.len() >= 1 {
                let backup_id = if self.prefix.is_empty() {
                    parts[0].to_string()
                } else {
                    // Skip the prefix part
                    if parts.len() >= 2 {
                        parts[1].to_string()
                    } else {
                        continue;
                    }
                };

                backup_ids.insert(backup_id);
            }
        }

        Ok(backup_ids.into_iter().collect())
    }

    /// Deletes a backup
    pub async fn delete_backup(&self, backup_id: &str) -> Result<(), StorageError> {
        info!("Deleting backup {}", backup_id);

        // Create the backup prefix
        let backup_prefix = if self.prefix.is_empty() {
            backup_id.to_string()
        } else {
            format!("{}/{}", self.prefix, backup_id)
        };

        // List all objects with the backup prefix
        let objects = self
            .provider
            .list_objects(&self.bucket, Some(&backup_prefix))
            .await?;

        if objects.is_empty() {
            return Err(StorageError::NotFound(format!(
                "No objects found for backup {}",
                backup_id
            )));
        }

        // Delete each object
        for obj in objects {
            self.provider.delete_object(&self.bucket, &obj.key).await?;
        }

        info!("Backup {} deleted successfully", backup_id);
        Ok(())
    }

    /// Generates a pre-signed URL for a backup file
    pub async fn generate_backup_file_url(
        &self,
        backup_id: &str,
        file_name: &str,
        expires_in: Duration,
    ) -> Result<String, StorageError> {
        // Create the backup key
        let key = if self.prefix.is_empty() {
            format!("{}/{}", backup_id, file_name)
        } else {
            format!("{}/{}/{}", self.prefix, backup_id, file_name)
        };

        // Generate the pre-signed URL
        self.provider
            .generate_presigned_url(&self.bucket, &key, expires_in)
            .await
    }
}
