use std::fmt;

/// Error type for storage operations
#[derive(Debug)]
pub enum StorageError {
    /// AWS SDK error
    Aws(String),
    /// AWS SDK error (alias for Aws for backward compatibility)
    AwsSdk(String),
    /// Authentication error
    Authentication(String),
    /// Configuration error
    Configuration(String),
    /// Google error
    Google(String),
    /// I/O error
    Io(std::io::Error),
    /// Object not found
    NotFound(String),
    /// Permission denied
    PermissionDenied(String),
    /// Request error
    Request(String),
    /// Serialization/deserialization error
    Serialization(String),
    /// Unexpected error
    Unexpected(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Aws(msg) => write!(f, "AWS SDK error: {}", msg),
            StorageError::AwsSdk(msg) => write!(f, "AWS SDK error: {}", msg),
            StorageError::Authentication(msg) => write!(f, "Authentication error: {}", msg),
            StorageError::Configuration(msg) => write!(f, "Configuration error: {}", msg),
            StorageError::Google(msg) => write!(f, "Google error: {}", msg),
            StorageError::Io(err) => write!(f, "I/O error: {}", err),
            StorageError::NotFound(msg) => write!(f, "Not found: {}", msg),
            StorageError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            StorageError::Request(msg) => write!(f, "Request error: {}", msg),
            StorageError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            StorageError::Unexpected(msg) => write!(f, "Unexpected error: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        StorageError::Io(err)
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::create_bucket::CreateBucketError>> for StorageError {
    fn from(err: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::create_bucket::CreateBucketError>) -> Self {
        StorageError::Aws(err.to_string())
    }
}

// This implementation is no longer needed as aws_smithy_types::error::Error doesn't exist in the current version
// Instead, we'll handle specific error types from the AWS SDK

impl From<reqwest::Error> for StorageError {
    fn from(err: reqwest::Error) -> Self {
        StorageError::Request(err.to_string())
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(err: serde_json::Error) -> Self {
        StorageError::Serialization(err.to_string())
    }
}
