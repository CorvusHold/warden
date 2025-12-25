//! Encryption module for backup and WAL data.
//!
//! Provides AES-256-GCM encryption with support for key loading from
//! environment variables or files.

mod cipher;
mod config;
mod format;
mod keys;

pub use cipher::{decrypt_data, decrypt_file, encrypt_data, encrypt_file};
pub use config::{EncryptionAlgorithm, EncryptionConfig};
pub use format::{EncryptedFileHeader, ENCRYPTED_FILE_MAGIC, FORMAT_VERSION};
pub use keys::{EncryptionKey, KeyError};

use thiserror::Error;

/// Errors that can occur during encryption operations.
#[derive(Debug, Error)]
pub enum EncryptionError {
    #[error("Key error: {0}")]
    KeyError(#[from] KeyError),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Invalid encrypted file format: {0}")]
    InvalidFormat(String),

    #[error("Authentication failed (wrong key or corrupted data)")]
    AuthenticationFailed,

    #[error("Unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
