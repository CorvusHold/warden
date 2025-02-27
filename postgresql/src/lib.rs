pub mod backup;
pub mod common;
pub mod manager;
pub mod restore;
pub mod user;
pub mod wrapper;

use thiserror::Error;
use uuid::Uuid;

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
    IoError(#[from] std::io::Error),

    #[error("Postgres error: {0}")]
    PostgresError(#[from] tokio_postgres::Error),
}

pub type Result<T> = std::result::Result<T, PostgresError>;

// Re-export key types for convenience
pub use common::{
    Backup, BackupCatalog, BackupStatus, BackupType, PostgresConfig, Restore, RestoreStatus,
};
pub use manager::PostgresManager;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
