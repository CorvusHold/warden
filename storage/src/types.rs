use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Represents a storage bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    /// Name of the bucket
    pub name: String,
    /// Creation time of the bucket
    pub creation_date: Option<SystemTime>,
    /// Region where the bucket is located
    pub region: Option<String>,
}

/// Represents an object in storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageObject {
    /// Key (path) of the object
    pub key: String,
    /// Size of the object in bytes
    pub size: Option<u64>,
    /// Last modified time
    pub last_modified: Option<DateTime<Utc>>,
    /// ETag of the object
    pub etag: Option<String>,
    /// Storage class of the object
    pub storage_class: Option<String>,
}

/// Represents metadata for an object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadata {
    /// Key (path) of the object
    pub key: String,
    /// Size of the object in bytes
    pub size: Option<u64>,
    /// Last modified time
    pub last_modified: Option<DateTime<Utc>>,
    /// ETag of the object
    pub etag: Option<String>,
    /// Content type of the object
    pub content_type: Option<String>,
    /// Storage class of the object
    pub storage_class: Option<String>,
    /// Custom metadata
    pub metadata: Option<Metadata>,
}

/// Custom metadata for objects
pub type Metadata = HashMap<String, String>;

/// Type of backup
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackupType {
    /// Full backup
    Full,
    /// Incremental backup
    Incremental,
    /// Snapshot backup
    Snapshot,
}

/// Information about a backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    /// Backup ID
    pub id: String,
    /// Type of backup
    pub backup_type: BackupType,
    /// Timestamp when the backup was created
    pub timestamp: DateTime<Utc>,
    /// Size of the backup in bytes
    pub size: u64,
    /// Parent backup ID (for incremental backups)
    pub parent_id: Option<String>,
}

/// Storage provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Provider type
    pub provider: StorageProviderType,
    /// Region for the provider
    pub region: Option<String>,
    /// Custom endpoint URL
    pub endpoint: Option<String>,
    /// Access key ID
    pub access_key: Option<String>,
    /// Secret access key
    pub secret_key: Option<String>,
    /// Account ID (for Cloudflare R2)
    pub account_id: Option<String>,
    /// Project ID (for Google Cloud Storage)
    pub project_id: Option<String>,
    /// Path to credentials file (for Google Cloud Storage)
    pub credentials_path: Option<String>,
}

/// Supported storage provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageProviderType {
    /// Amazon S3
    #[serde(rename = "s3")]
    S3,
}

/// Streaming upload options
#[derive(Debug, Clone)]
pub struct StreamingUploadOptions {
    /// Content type of the data
    pub content_type: Option<String>,
    /// Custom metadata
    pub metadata: Option<Metadata>,
    /// Part size for multipart uploads (in bytes)
    pub part_size: Option<usize>,
}

impl Default for StreamingUploadOptions {
    fn default() -> Self {
        Self {
            content_type: None,
            metadata: None,
            part_size: Some(5 * 1024 * 1024), // 5 MB default part size (S3 minimum)
        }
    }
}

/// Streaming download options
#[derive(Debug, Clone, Default)]
pub struct StreamingDownloadOptions {
    /// Range start (in bytes)
    pub range_start: Option<u64>,
    /// Range end (in bytes)
    pub range_end: Option<u64>,
}
