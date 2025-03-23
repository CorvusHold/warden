pub mod backup;
pub mod cli;
pub mod common;
pub mod manager;
pub mod restore;
pub mod user;
pub mod wrapper;

use thiserror::Error;
use anyhow;

#[derive(Error, Debug)]
pub enum PostgresError {
    #[error("Database connection error: {0}")]
    ConnectionError(String),

    #[error("Backup error: {0}")]
    BackupError(String),

    #[error("Backup not found: {0}")]
    BackupNotFound(uuid::Uuid),

    #[error("Restore error: {0}")]
    RestoreError(String),

    #[error("WAL error: {0}")]
    WalError(String),

    #[error("Permission error: {0}")]
    PermissionError(String),

    #[error("IO error: {0}")]
    Io(std::io::Error),

    #[error("Postgres error: {0}")]
    Postgres(tokio_postgres::Error),

    #[error("Missing password")]
    MissingPassword,

    #[error("Anyhow error: {0}")]
    Anyhow(anyhow::Error),
}

impl From<std::io::Error> for PostgresError {
    fn from(err: std::io::Error) -> Self {
        PostgresError::Io(err)
    }
}

impl From<tokio_postgres::Error> for PostgresError {
    fn from(err: tokio_postgres::Error) -> Self {
        PostgresError::Postgres(err)
    }
}

impl From<anyhow::Error> for PostgresError {
    fn from(err: anyhow::Error) -> Self {
        PostgresError::Anyhow(err)
    }
}

pub type Result<T> = std::result::Result<T, PostgresError>;

// Re-export key types for convenience
pub use common::{
    Backup, BackupCatalog, BackupStatus, BackupType, PostgresConfig, Restore, RestoreStatus,
};
pub use manager::PostgresManager;

#[cfg(test)]
mod tests {

    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
