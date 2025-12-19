use crate::{
    BackupInfo, BackupType, Metadata, StorageError, StorageProvider, StorageProviderFactory,
    StorageProviderType,
};
use log::{error, info};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

/// Integration with PostgreSQL backup system
#[derive(Clone)]
pub struct PostgresBackupStorage {
    /// Storage provider
    pub(crate) provider: Arc<dyn StorageProvider>,
    /// Bucket name
    pub(crate) bucket: String,
    /// Base prefix for backups
    pub(crate) prefix: String,
}

impl PostgresBackupStorage {
    /// Creates a new PostgreSQL backup storage
    #[allow(clippy::too_many_arguments)]
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
        let provider: Arc<dyn StorageProvider> = match provider_type {
            StorageProviderType::S3 => Arc::from(
                StorageProviderFactory::create_s3_provider(region, endpoint, access_key, secret_key)
                    .await?,
            ),
        };

        // Try to create the bucket regardless of whether it exists
        // This is a workaround for the "service error" issue
        info!("Attempting to create bucket {bucket} if it doesn't exist");
        match provider.create_bucket(&bucket).await {
            Ok(_) => {}
            Err(e) => {
                // If bucket already exists, that's fine
                if e.to_string().contains("BucketAlreadyOwnedByYou")
                    || e.to_string().contains("BucketAlreadyExists")
                {
                    info!("Bucket {bucket} already exists");
                } else {
                    // Log the error but continue - we'll try to use the bucket anyway
                    error!("Failed to create bucket {bucket}: {e}");
                    // Don't return the error, try to proceed anyway
                }
            }
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
        info!(
            "Uploading backup {} from {}",
            backup_id,
            backup_path.display()
        );

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

                info!("Uploading file: {} ({})", rel_path.display(), key);
                // --- Sentry scope for upload ---
                sentry::configure_scope(|scope| {
                    scope.set_tag("operation", "upload_backup");
                    scope.set_tag("backup_id", backup_id);
                    scope.set_tag("file", rel_path.to_string_lossy());
                    scope.set_tag("bucket", &self.bucket);
                    scope.set_tag("key", &key);
                    scope.set_tag(
                        "region",
                        std::env::var("AWS_REGION").unwrap_or_else(|_| "unknown".into()),
                    );
                    scope.set_tag("endpoint", option_env!("AWS_ENDPOINT").unwrap_or("unknown"));
                });
                // --- End Sentry scope ---
                self.provider
                    .upload_file(
                        &self.bucket,
                        &key,
                        entry.path(),
                        content_type,
                        metadata.clone(),
                    )
                    .await?;
            }
        }

        info!("Backup {backup_id} uploaded successfully");
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
        info!("Streaming upload of backup file {file_name} for backup {backup_id}");

        // Create the backup key
        let key = if self.prefix.is_empty() {
            format!("{backup_id}/{file_name}")
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

        // --- Sentry scope for upload_stream ---
        sentry::configure_scope(|scope| {
            scope.set_tag("operation", "upload_backup_stream");
            scope.set_tag("backup_id", backup_id);
            scope.set_tag("file", file_name);
            scope.set_tag("bucket", &self.bucket);
            scope.set_tag("key", &key);
            scope.set_tag(
                "region",
                std::env::var("AWS_REGION").unwrap_or_else(|_| "unknown".into()),
            );
            scope.set_tag("endpoint", option_env!("AWS_ENDPOINT").unwrap_or("unknown"));
        });
        // --- End Sentry scope ---
        self.provider
            .upload_stream(&self.bucket, &key, Box::pin(stream), content_type, metadata)
            .await?;

        info!("Backup file {file_name} streamed successfully");
        Ok(())
    }

    /// Uploads a physical backup using pg_basebackup
    pub async fn upload_physical_backup(
        &self,
        backup_id: &str,
        backup_path: &Path,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError> {
        info!(
            "Uploading physical backup {} from {}",
            backup_id,
            backup_path.display()
        );

        // Check if the backup path exists
        if !backup_path.exists() {
            error!("Backup path does not exist: {}", backup_path.display());
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Backup path does not exist: {}", backup_path.display()),
            )));
        }

        // Check if the backup path is a directory
        if !backup_path.is_dir() {
            error!("Backup path is not a directory: {}", backup_path.display());
            return Err(StorageError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Backup path is not a directory: {}", backup_path.display()),
            )));
        }

        // List all files in the backup directory
        let backup_files = match std::fs::read_dir(backup_path) {
            Ok(files) => files,
            Err(e) => {
                error!(
                    "Failed to read backup directory {}: {}",
                    backup_path.display(),
                    e
                );
                return Err(StorageError::Io(e));
            }
        };

        // Upload each file in the backup directory
        for file_result in backup_files {
            let file = match file_result {
                Ok(f) => f,
                Err(e) => {
                    error!("Failed to read file in backup directory: {e}");
                    return Err(StorageError::Io(e));
                }
            };

            let file_path = file.path();
            if file_path.is_file() {
                let file_name = file.file_name().to_string_lossy().to_string();
                info!("Uploading file: {} ({})", file_name, file_path.display());
                match self
                    .upload_backup_stream(backup_id, &file_name, &file_path, metadata.clone())
                    .await
                {
                    Ok(_) => info!("Successfully uploaded file: {file_name}"),
                    Err(e) => {
                        error!("Failed to upload file {file_name}: {e}");
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    /// Uploads a logical backup using pg_dump
    pub async fn upload_logical_backup(
        &self,
        backup_id: &str,
        dump_file: &Path,
        metadata: Option<Metadata>,
    ) -> Result<(), StorageError> {
        self.upload_backup_stream(backup_id, "pg_dump.dump", dump_file, metadata)
            .await
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
                "No objects found for backup {backup_id}"
            )));
        }

        // Create the target directory if it doesn't exist
        tokio::fs::create_dir_all(target_dir)
            .await
            .map_err(StorageError::Io)?;

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
                    .map_err(StorageError::Io)?;
            }

            self.provider
                .download_file(&self.bucket, &obj.key, &target_path)
                .await?;
        }

        info!("Backup {backup_id} downloaded successfully");
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
            format!("{backup_id}/{file_name}")
        } else {
            format!("{}/{}/{}", self.prefix, backup_id, file_name)
        };

        // Create parent directories if they don't exist
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(StorageError::Io)?;
        }

        // Download the file
        self.provider
            .download_file(&self.bucket, &key, target_path)
            .await?;

        info!("Backup file {file_name} downloaded successfully");
        Ok(())
    }

    /// Lists all backups
    pub async fn list_backups(&self) -> Result<Vec<BackupInfo>, StorageError> {
        let prefix = if self.prefix.is_empty() {
            None
        } else {
            Some(self.prefix.as_str())
        };

        let objects = self.provider.list_objects(&self.bucket, prefix).await?;

        // Extract unique backup IDs from object keys
        let mut backup_ids = std::collections::HashSet::new();
        let mut backup_infos = Vec::new();

        for obj in objects {
            let key = obj.key;
            let parts: Vec<&str> = key.split('/').collect();

            if !parts.is_empty() {
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

                if backup_ids.insert(backup_id.clone()) {
                    // Get metadata file if it exists
                    let _metadata_key = if self.prefix.is_empty() {
                        format!("{backup_id}/metadata.json")
                    } else {
                        format!("{}/{}/metadata.json", self.prefix, backup_id)
                    };

                    let backup_type = if key.contains("snapshot") {
                        BackupType::Snapshot
                    } else if key.contains("incremental") {
                        BackupType::Incremental
                    } else {
                        BackupType::Full
                    };

                    let timestamp = obj.last_modified.unwrap_or_else(chrono::Utc::now);

                    backup_infos.push(BackupInfo {
                        id: backup_id,
                        backup_type,
                        timestamp,
                        size: obj.size.unwrap_or(0),
                        parent_id: None, // Would need to parse metadata to get this
                    });
                }
            }
        }

        Ok(backup_infos)
    }

    /// Lists all backups that have a specific backup as an ancestor
    pub async fn list_backups_with_ancestor(
        &self,
        _ancestor_id: &str,
    ) -> Result<Vec<String>, StorageError> {
        // Get all backups
        let all_backups = self.list_backups().await?;
        let mut incremental_backups = Vec::new();

        // For a proper implementation, we would need to parse metadata files to determine ancestry
        // This is a simplified version that just returns all incremental backups
        for backup in all_backups {
            if backup.backup_type == BackupType::Incremental {
                incremental_backups.push(backup.id);
            }
        }

        Ok(incremental_backups)
    }

    /// Deletes a backup
    pub async fn delete_backup(&self, backup_id: &str) -> Result<(), StorageError> {
        info!("Deleting backup {backup_id}");

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
                "No objects found for backup {backup_id}"
            )));
        }

        // Delete each object
        for obj in objects {
            self.provider.delete_object(&self.bucket, &obj.key).await?;
        }

        info!("Backup {backup_id} deleted successfully");
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
            format!("{backup_id}/{file_name}")
        } else {
            format!("{}/{}/{}", self.prefix, backup_id, file_name)
        };

        // Generate the pre-signed URL
        self.provider
            .generate_presigned_url(&self.bucket, &key, expires_in)
            .await
    }

    /// Creates backup metadata from a backup directory
    #[allow(clippy::too_many_arguments)]
    pub async fn create_backup_metadata(
        &self,
        backup_id: &str,
        backup_path: &std::path::Path,
        backup_type: BackupType,
        status: crate::BackupStatus,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        base_backup_id: Option<String>,
        wal_start: Option<String>,
        wal_end: Option<String>,
        server_version: String,
    ) -> Result<crate::BackupMetadata, StorageError> {
        use sha2::{Digest, Sha256};
        use std::io::Read;

        let mut total_size = 0u64;
        let mut files = Vec::new();
        let mut hasher = Sha256::new();
        let mut aggregate_has_bytes = false;

        // Walk through the backup directory
        let walker = walkdir::WalkDir::new(backup_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok());

        for entry in walker {
            if entry.file_type().is_file() {
                let file_path = entry.path();
                let rel_path = file_path
                    .strip_prefix(backup_path)
                    .map_err(|e| StorageError::Unexpected(e.to_string()))?;

                let metadata = std::fs::metadata(file_path).map_err(StorageError::Io)?;
                let file_size = metadata.len();
                total_size += file_size;

                // Calculate file checksum
                let file_checksum = if file_size < 100 * 1024 * 1024 {
                    // Only calculate checksum for files < 100MB
                    let mut file = std::fs::File::open(file_path).map_err(StorageError::Io)?;
                    let mut file_hasher = Sha256::new();
                    let mut buffer = vec![0u8; 8192];
                    loop {
                        let n = file.read(&mut buffer).map_err(StorageError::Io)?;
                        if n == 0 {
                            break;
                        }
                        file_hasher.update(&buffer[..n]);
                        hasher.update(&buffer[..n]);
                        aggregate_has_bytes = true;
                    }
                    Some(format!("{:x}", file_hasher.finalize()))
                } else {
                    None
                };

                files.push(crate::BackupFile {
                    name: rel_path.to_string_lossy().to_string(),
                    size: file_size,
                    checksum: file_checksum,
                });
            }
        }

        let checksum = if aggregate_has_bytes {
            Some(format!("{:x}", hasher.finalize()))
        } else {
            None
        };

        Ok(crate::BackupMetadata {
            id: backup_id.to_string(),
            backup_type,
            status,
            start_time,
            end_time,
            base_backup_id,
            wal_start,
            wal_end,
            size_bytes: total_size,
            server_version,
            checksum,
            files,
            tags: Vec::new(),
            pinned: false,
            encrypted: None, // Will be set by caller if encryption is enabled
            encryption_algorithm: None,
        })
    }

    /// Uploads backup metadata to storage
    pub async fn upload_backup_metadata(
        &self,
        backup_id: &str,
        metadata: &crate::BackupMetadata,
    ) -> Result<(), StorageError> {
        let key = if self.prefix.is_empty() {
            format!("{}/backup_metadata.json", backup_id)
        } else {
            format!("{}/{}/backup_metadata.json", self.prefix, backup_id)
        };

        let json = serde_json::to_string_pretty(metadata)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let temp_file = tempfile::NamedTempFile::new().map_err(StorageError::Io)?;
        std::fs::write(temp_file.path(), json).map_err(StorageError::Io)?;

        self.provider
            .upload_file(
                &self.bucket,
                &key,
                temp_file.path(),
                Some("application/json"),
                None,
            )
            .await?;

        info!("Uploaded backup metadata for {}", backup_id);
        Ok(())
    }

    /// Gets backup metadata from remote storage
    pub async fn get_remote_backup_metadata(
        &self,
        backup_id: &str,
    ) -> Result<crate::BackupMetadata, StorageError> {
        let key = if self.prefix.is_empty() {
            format!("{}/backup_metadata.json", backup_id)
        } else {
            format!("{}/{}/backup_metadata.json", self.prefix, backup_id)
        };

        let temp_file = tempfile::NamedTempFile::new().map_err(StorageError::Io)?;

        self.provider
            .download_file(&self.bucket, &key, temp_file.path())
            .await?;

        let json = std::fs::read_to_string(temp_file.path()).map_err(StorageError::Io)?;

        let metadata: crate::BackupMetadata = serde_json::from_str(&json)
            .map_err(|e| StorageError::Unexpected(format!("Failed to parse metadata: {}", e)))?;

        Ok(metadata)
    }

    /// Lists all objects in the storage bucket (raw storage API)
    pub async fn list_all_objects(&self) -> Result<Vec<crate::StorageObject>, StorageError> {
        let prefix = if self.prefix.is_empty() {
            None
        } else {
            Some(self.prefix.as_str())
        };

        self.provider.list_objects(&self.bucket, prefix).await
    }

    /// Lists all remote backups with detailed metadata
    pub async fn list_remote_backups_detailed(
        &self,
    ) -> Result<Vec<crate::BackupMetadata>, StorageError> {
        let prefix = if self.prefix.is_empty() {
            None
        } else {
            Some(self.prefix.as_str())
        };

        let objects = self.provider.list_objects(&self.bucket, prefix).await?;

        // Find all backup_metadata.json files
        let metadata_keys: Vec<_> = objects
            .iter()
            .filter(|obj| obj.key.ends_with("/backup_metadata.json"))
            .collect();

        let mut backups = Vec::new();

        for obj in metadata_keys {
            // Extract backup ID from key
            let parts: Vec<&str> = obj.key.split('/').collect();
            let backup_id = if self.prefix.is_empty() {
                parts.first().map(|s| s.to_string())
            } else {
                parts.get(1).map(|s| s.to_string())
            };

            if let Some(backup_id) = backup_id {
                match self.get_remote_backup_metadata(&backup_id).await {
                    Ok(metadata) => backups.push(metadata),
                    Err(e) => {
                        error!("Failed to load metadata for backup {}: {}", backup_id, e);
                    }
                }
            }
        }

        // Sort by timestamp, newest first
        backups.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        Ok(backups)
    }

    /// Loads retention policy from bucket root
    pub async fn load_retention_policy(
        &self,
    ) -> Result<Option<crate::RetentionPolicy>, StorageError> {
        let key = if self.prefix.is_empty() {
            "retention_policy.json".to_string()
        } else {
            format!("{}/retention_policy.json", self.prefix)
        };

        // Check if policy file exists
        if !self.provider.object_exists(&self.bucket, &key).await? {
            return Ok(None);
        }

        let temp_file = tempfile::NamedTempFile::new().map_err(StorageError::Io)?;

        self.provider
            .download_file(&self.bucket, &key, temp_file.path())
            .await?;

        let json = std::fs::read_to_string(temp_file.path()).map_err(StorageError::Io)?;

        let policy: crate::RetentionPolicy = serde_json::from_str(&json).map_err(|e| {
            StorageError::Unexpected(format!("Failed to parse retention policy: {}", e))
        })?;

        Ok(Some(policy))
    }

    /// Saves retention policy to bucket root
    pub async fn save_retention_policy(
        &self,
        policy: &crate::RetentionPolicy,
    ) -> Result<(), StorageError> {
        let key = if self.prefix.is_empty() {
            "retention_policy.json".to_string()
        } else {
            format!("{}/retention_policy.json", self.prefix)
        };

        let json = serde_json::to_string_pretty(policy)
            .map_err(|e| StorageError::Unexpected(e.to_string()))?;

        let temp_file = tempfile::NamedTempFile::new().map_err(StorageError::Io)?;
        std::fs::write(temp_file.path(), json).map_err(StorageError::Io)?;

        self.provider
            .upload_file(
                &self.bucket,
                &key,
                temp_file.path(),
                Some("application/json"),
                None,
            )
            .await?;

        info!("Saved retention policy to bucket {}", self.bucket);
        Ok(())
    }

    /// Evaluates which backups to purge according to the retention policy
    pub async fn evaluate_purge(
        &self,
        policy: &crate::RetentionPolicy,
    ) -> Result<crate::PurgeEvaluation, StorageError> {
        info!("Evaluating purge policy for bucket {}", self.bucket);

        // List all remote backups with metadata
        let backups = self.list_remote_backups_detailed().await?;

        // Evaluate using the purge module
        let evaluation = crate::purge::evaluate_retention_policy(&backups, policy)?;

        info!(
            "Purge evaluation complete: {} to keep, {} to delete, {} bytes to free",
            evaluation.to_keep.len(),
            evaluation.to_delete.len(),
            evaluation.estimated_space_freed
        );

        Ok(evaluation)
    }

    /// Executes a purge operation (DELETES backups from remote storage)
    pub async fn execute_purge(
        &self,
        evaluation: &crate::PurgeEvaluation,
        dry_run: bool,
    ) -> Result<crate::PurgeReport, StorageError> {
        let start_time = std::time::Instant::now();
        let mut deleted = 0;
        let mut failed = 0;
        let mut errors = Vec::new();
        let mut space_freed = 0u64;

        if dry_run {
            info!(
                "DRY RUN: Would delete {} backups and free {} bytes",
                evaluation.to_delete.len(),
                evaluation.estimated_space_freed
            );
            return Ok(crate::PurgeReport {
                timestamp: chrono::Utc::now(),
                dry_run: true,
                total_evaluated: evaluation.total_backups,
                kept: evaluation.to_keep.len(),
                deleted: 0,
                failed: 0,
                space_freed: 0,
                duration_secs: start_time.elapsed().as_secs(),
                errors: Vec::new(),
            });
        }

        info!(
            "Executing purge: deleting {} backups",
            evaluation.to_delete.len()
        );

        // Delete each backup
        for decision in &evaluation.to_delete {
            info!(
                "Deleting backup {}: {}",
                decision.backup_id, decision.reason
            );

            match self.delete_backup(&decision.backup_id).await {
                Ok(_) => {
                    deleted += 1;
                    space_freed += decision.size_bytes;
                    info!("Successfully deleted backup {}", decision.backup_id);
                }
                Err(e) => {
                    failed += 1;
                    let error_msg =
                        format!("Failed to delete backup {}: {}", decision.backup_id, e);
                    error!("{}", error_msg);

                    // Report to Sentry
                    sentry::capture_message(&error_msg, sentry::Level::Error);

                    errors.push(error_msg);
                }
            }
        }

        let duration_secs = start_time.elapsed().as_secs();

        info!(
            "Purge complete: deleted {}, failed {}, freed {} bytes in {}s",
            deleted, failed, space_freed, duration_secs
        );

        Ok(crate::PurgeReport {
            timestamp: chrono::Utc::now(),
            dry_run: false,
            total_evaluated: evaluation.total_backups,
            kept: evaluation.to_keep.len(),
            deleted,
            failed,
            space_freed,
            duration_secs,
            errors,
        })
    }
}
