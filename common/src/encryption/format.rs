//! Encrypted file format handling.
//!
//! File format:
//! ```text
//! +------------------+
//! | Magic (8 bytes)  |  "WARDEN01"
//! +------------------+
//! | Version (1 byte) |  0x01
//! +------------------+
//! | Algorithm (1 b)  |  0x01 = AES-256-GCM
//! +------------------+
//! | Nonce (12 bytes) |  Random nonce
//! +------------------+
//! | Reserved (10 b)  |  Future use
//! +------------------+
//! | Ciphertext       |  Encrypted data + GCM tag
//! +------------------+
//! ```

use super::config::EncryptionAlgorithm;
use super::EncryptionError;

/// Magic bytes identifying an encrypted Warden file.
pub const ENCRYPTED_FILE_MAGIC: &[u8; 8] = b"WARDEN01";

/// Current format version.
pub const FORMAT_VERSION: u8 = 0x01;

/// Total header size in bytes.
pub const HEADER_SIZE: usize = 8 + 1 + 1 + 12 + 10; // 32 bytes

/// Header for encrypted files.
#[derive(Debug, Clone)]
pub struct EncryptedFileHeader {
    /// Format version.
    pub version: u8,
    /// Encryption algorithm.
    pub algorithm: EncryptionAlgorithm,
    /// Nonce/IV for encryption.
    pub nonce: [u8; 12],
    /// Reserved bytes for future use.
    pub reserved: [u8; 10],
}

impl EncryptedFileHeader {
    /// Create a new header with a random nonce.
    pub fn new(algorithm: EncryptionAlgorithm) -> Self {
        use rand::RngCore;
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);

        Self {
            version: FORMAT_VERSION,
            algorithm,
            nonce,
            reserved: [0u8; 10],
        }
    }

    /// Create a header with a specific nonce (for testing).
    #[cfg(test)]
    pub fn with_nonce(algorithm: EncryptionAlgorithm, nonce: [u8; 12]) -> Self {
        Self {
            version: FORMAT_VERSION,
            algorithm,
            nonce,
            reserved: [0u8; 10],
        }
    }

    /// Serialize the header to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_SIZE);
        bytes.extend_from_slice(ENCRYPTED_FILE_MAGIC);
        bytes.push(self.version);
        bytes.push(self.algorithm.format_id());
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.reserved);
        bytes
    }

    /// Parse a header from bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, EncryptionError> {
        if data.len() < HEADER_SIZE {
            return Err(EncryptionError::InvalidFormat(format!(
                "Data too short for header: {} bytes, need {}",
                data.len(),
                HEADER_SIZE
            )));
        }

        // Check magic
        if &data[0..8] != ENCRYPTED_FILE_MAGIC {
            return Err(EncryptionError::InvalidFormat(
                "Invalid magic bytes - file is not encrypted or corrupted".to_string(),
            ));
        }

        // Check version
        let version = data[8];
        if version != FORMAT_VERSION {
            return Err(EncryptionError::InvalidFormat(format!(
                "Unsupported format version: {}",
                version
            )));
        }

        // Parse algorithm
        let algorithm = EncryptionAlgorithm::from_format_id(data[9]).ok_or_else(|| {
            EncryptionError::UnsupportedAlgorithm(format!("Unknown algorithm ID: {}", data[9]))
        })?;

        // Parse nonce
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&data[10..22]);

        // Parse reserved
        let mut reserved = [0u8; 10];
        reserved.copy_from_slice(&data[22..32]);

        Ok(Self {
            version,
            algorithm,
            nonce,
            reserved,
        })
    }
}

/// Check if data appears to be encrypted (has valid magic header).
#[allow(dead_code)] // Public API for encryption detection
pub fn is_encrypted(data: &[u8]) -> bool {
    data.len() >= 8 && &data[0..8] == ENCRYPTED_FILE_MAGIC
}

/// Check if a file appears to be encrypted.
#[allow(dead_code)] // Public API for encryption detection
pub fn is_file_encrypted(path: &std::path::Path) -> std::io::Result<bool> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 8];

    match file.read_exact(&mut magic) {
        Ok(()) => Ok(&magic == ENCRYPTED_FILE_MAGIC),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = EncryptedFileHeader::new(EncryptionAlgorithm::Aes256Gcm);
        let bytes = header.to_bytes();

        assert_eq!(bytes.len(), HEADER_SIZE);

        let parsed = EncryptedFileHeader::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.version, header.version);
        assert_eq!(parsed.algorithm, header.algorithm);
        assert_eq!(parsed.nonce, header.nonce);
    }

    #[test]
    fn test_header_magic() {
        let header = EncryptedFileHeader::new(EncryptionAlgorithm::Aes256Gcm);
        let bytes = header.to_bytes();

        assert_eq!(&bytes[0..8], ENCRYPTED_FILE_MAGIC);
    }

    #[test]
    fn test_invalid_magic() {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[0..8].copy_from_slice(b"INVALID!");

        let result = EncryptedFileHeader::from_bytes(&bytes);
        assert!(matches!(result, Err(EncryptionError::InvalidFormat(_))));
    }

    #[test]
    fn test_is_encrypted() {
        let header = EncryptedFileHeader::new(EncryptionAlgorithm::Aes256Gcm);
        let bytes = header.to_bytes();

        assert!(is_encrypted(&bytes));
        assert!(!is_encrypted(b"plaintext data"));
        assert!(!is_encrypted(&[]));
    }

    #[test]
    fn test_too_short() {
        let result = EncryptedFileHeader::from_bytes(&[0u8; 10]);
        assert!(matches!(result, Err(EncryptionError::InvalidFormat(_))));
    }
}
