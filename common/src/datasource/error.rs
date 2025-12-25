//! Error types for the data source plugin system.

use std::fmt;
use thiserror::Error;
use uuid::Uuid;

/// Errors that can occur in data source operations
#[derive(Error, Debug)]
pub enum DataSourceError {
    /// Connection to the data source failed
    #[error("Connection error: {0}")]
    Connection(String),

    /// Authentication failed
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Backup operation failed
    #[error("Backup error: {0}")]
    Backup(String),

    /// Restore operation failed
    #[error("Restore error: {0}")]
    Restore(String),

    /// PITR is not supported by this data source
    #[error("Point-in-Time Recovery is not supported by this data source")]
    PitrNotSupported,

    /// PITR operation failed
    #[error("PITR error: {0}")]
    Pitr(String),

    /// Requested backup was not found
    #[error("Backup not found: {0}")]
    BackupNotFound(Uuid),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Storage operation failed
    #[error("Storage error: {0}")]
    Storage(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// SSH tunnel error
    #[error("SSH tunnel error: {0}")]
    SshTunnel(String),

    /// Operation timed out
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Operation was cancelled
    #[error("Operation cancelled: {0}")]
    Cancelled(String),

    /// Feature not supported
    #[error("Feature not supported: {0}")]
    NotSupported(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl DataSourceError {
    /// Create a connection error
    pub fn connection(msg: impl Into<String>) -> Self {
        Self::Connection(msg.into())
    }

    /// Create an authentication error
    pub fn authentication(msg: impl Into<String>) -> Self {
        Self::Authentication(msg.into())
    }

    /// Create a backup error
    pub fn backup(msg: impl Into<String>) -> Self {
        Self::Backup(msg.into())
    }

    /// Create a restore error
    pub fn restore(msg: impl Into<String>) -> Self {
        Self::Restore(msg.into())
    }

    /// Create a configuration error
    pub fn configuration(msg: impl Into<String>) -> Self {
        Self::Configuration(msg.into())
    }

    /// Create a storage error
    pub fn storage(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }

    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Check if this is a retryable error
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Connection(_) | Self::Timeout(_) | Self::SshTunnel(_)
        )
    }

    /// Check if this is a configuration error
    pub fn is_configuration_error(&self) -> bool {
        matches!(self, Self::Configuration(_) | Self::Authentication(_))
    }
}

/// Errors related to plugin management
#[derive(Error, Debug)]
pub enum PluginError {
    /// Plugin with this name already exists
    #[error("Plugin already registered: {0}")]
    AlreadyRegistered(String),

    /// Plugin not found
    #[error("Plugin not found: {0}")]
    NotFound(String),

    /// Plugin initialization failed
    #[error("Plugin initialization failed: {0}")]
    InitializationFailed(String),

    /// Plugin version mismatch
    #[error("Plugin version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: String, actual: String },

    /// Invalid plugin
    #[error("Invalid plugin: {0}")]
    Invalid(String),
}

impl PluginError {
    /// Create an already registered error
    pub fn already_registered(name: impl Into<String>) -> Self {
        Self::AlreadyRegistered(name.into())
    }

    /// Create a not found error
    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound(name.into())
    }

    /// Create an initialization failed error
    pub fn initialization_failed(msg: impl Into<String>) -> Self {
        Self::InitializationFailed(msg.into())
    }
}

/// Result type for data source operations
pub type DataSourceResult<T> = Result<T, DataSourceError>;

/// Result type for plugin operations
pub type PluginResult<T> = Result<T, PluginError>;

/// Error context for better error messages
#[derive(Debug)]
pub struct ErrorContext {
    /// The operation that was being performed
    pub operation: String,
    /// The data source type
    pub datasource: Option<String>,
    /// Additional context
    pub context: Vec<(String, String)>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            datasource: None,
            context: Vec::new(),
        }
    }

    /// Set the data source
    pub fn with_datasource(mut self, datasource: impl Into<String>) -> Self {
        self.datasource = Some(datasource.into());
        self
    }

    /// Add context
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.push((key.into(), value.into()));
        self
    }
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Operation: {}", self.operation)?;
        if let Some(ds) = &self.datasource {
            write!(f, ", DataSource: {}", ds)?;
        }
        for (key, value) in &self.context {
            write!(f, ", {}: {}", key, value)?;
        }
        Ok(())
    }
}
