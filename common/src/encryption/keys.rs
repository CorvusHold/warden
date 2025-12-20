//! Encryption key management.

use crate::secrets::{SecretError, SecretRef};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use std::fmt;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::config::EncryptionAlgorithm;

/// Errors related to encryption keys.
#[derive(Debug, Error)]
pub enum KeyError {
    #[error("Key not found: {0}")]
    NotFound(String),

    #[error("Invalid key size: expected {expected} bytes, got {actual}")]
    InvalidKeySize { expected: usize, actual: usize },

    #[error("Invalid key format: {0}")]
    InvalidFormat(String),

    #[error("Secret resolution failed: {0}")]
    SecretError(#[from] SecretError),
}

/// An encryption key that is zeroized when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EncryptionKey {
    /// Raw key bytes (32 bytes for AES-256).
    bytes: Vec<u8>,
}

impl EncryptionKey {
    /// Create a key from raw bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, KeyError> {
        let expected = EncryptionAlgorithm::Aes256Gcm.key_size();
        if bytes.len() != expected {
            return Err(KeyError::InvalidKeySize {
                expected,
                actual: bytes.len(),
            });
        }
        Ok(Self { bytes })
    }

    /// Create a key from a base64-encoded string.
    pub fn from_base64(encoded: &str) -> Result<Self, KeyError> {
        let bytes = BASE64
            .decode(encoded.trim())
            .map_err(|e| KeyError::InvalidFormat(format!("Invalid base64: {}", e)))?;
        Self::from_bytes(bytes)
    }

    /// Create a key from a hex-encoded string.
    pub fn from_hex(hex: &str) -> Result<Self, KeyError> {
        let hex = hex.trim();
        let expected_hex_len = EncryptionAlgorithm::Aes256Gcm.key_size() * 2;
        if hex.len() != expected_hex_len {
            return Err(KeyError::InvalidFormat(format!(
                "Hex key must be {} characters ({} bytes), got {}",
                expected_hex_len,
                expected_hex_len / 2,
                hex.len()
            )));
        }

        let bytes: Result<Vec<u8>, _> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
            .collect();

        let bytes = bytes.map_err(|e| KeyError::InvalidFormat(format!("Invalid hex: {}", e)))?;
        Self::from_bytes(bytes)
    }

    /// Load a key from a secret reference.
    ///
    /// The secret value can be:
    /// - Raw 32 bytes (UTF-8 text; the key is taken from the string's UTF-8 bytes)
    /// - Base64-encoded key (44 characters)
    /// - Hex-encoded key (64 characters)
    pub fn from_secret_ref(secret_ref: &str) -> Result<Self, KeyError> {
        let ref_obj = SecretRef::new(secret_ref);
        let resolved = ref_obj.resolve()?;
        let value = resolved.expose();

        // Try to detect format
        let trimmed = value.trim();

        // Check if it's hex (64 hex chars = 32 bytes)
        if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Self::from_hex(trimmed);
        }

        // Check if it's base64 (44 chars for 32 bytes)
        if trimmed.len() == 44
            && trimmed
                .chars()
                .all(|c| c.is_alphanumeric() || c == '+' || c == '/' || c == '=')
        {
            return Self::from_base64(trimmed);
        }

        // Try base64 anyway (might have different padding)
        if let Ok(key) = Self::from_base64(trimmed) {
            return Ok(key);
        }

        // Treat as raw bytes
        let bytes = trimmed.as_bytes().to_vec();
        Self::from_bytes(bytes)
    }

    /// Get the key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Generate a random key for testing.
    #[cfg(test)]
    pub fn generate_random() -> Self {
        use rand::rngs::OsRng;
        use rand::RngCore;
        let mut bytes = vec![0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self { bytes }
    }
}

impl fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EncryptionKey([REDACTED {} bytes])", self.bytes.len())
    }
}

impl fmt::Display for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED_KEY]")
    }
}

/// Generate a new random encryption key.
#[allow(dead_code)] // Public API for key generation
pub fn generate_key() -> EncryptionKey {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = vec![0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    EncryptionKey { bytes }
}

/// Encode a key as base64 for storage.
#[allow(dead_code)] // Public API for key encoding
pub fn encode_key_base64(key: &EncryptionKey) -> String {
    BASE64.encode(key.as_bytes())
}

/// Encode a key as hex for storage.
#[allow(dead_code)] // Public API for key encoding
pub fn encode_key_hex(key: &EncryptionKey) -> String {
    key.as_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_from_bytes() {
        let bytes = vec![0u8; 32];
        let key = EncryptionKey::from_bytes(bytes).unwrap();
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn test_key_wrong_size() {
        let bytes = vec![0u8; 16];
        let result = EncryptionKey::from_bytes(bytes);
        assert!(matches!(result, Err(KeyError::InvalidKeySize { .. })));
    }

    #[test]
    fn test_key_from_base64() {
        // 32 bytes encoded as base64
        let key_bytes = [0x42u8; 32];
        let encoded = BASE64.encode(key_bytes);
        let key = EncryptionKey::from_base64(&encoded).unwrap();
        assert_eq!(key.as_bytes(), &key_bytes);
    }

    #[test]
    fn test_key_from_hex() {
        let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let key = EncryptionKey::from_hex(hex).unwrap();
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn test_key_debug_redacted() {
        let key = EncryptionKey::generate_random();
        let debug = format!("{:?}", key);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains(&format!("{:02x}", key.as_bytes()[0])));
    }

    #[test]
    fn test_generate_and_encode() {
        let key = generate_key();

        let b64 = encode_key_base64(&key);
        let key2 = EncryptionKey::from_base64(&b64).unwrap();
        assert_eq!(key.as_bytes(), key2.as_bytes());

        let hex = encode_key_hex(&key);
        let key3 = EncryptionKey::from_hex(&hex).unwrap();
        assert_eq!(key.as_bytes(), key3.as_bytes());
    }

    #[test]
    fn test_key_from_env_secret() {
        // Generate a test key and encode it
        let test_key = generate_key();
        let encoded = encode_key_base64(&test_key);

        // Set env var
        unsafe {
            std::env::set_var("WARDEN_TEST_KEY", &encoded);
        }

        let key = EncryptionKey::from_secret_ref("env:WARDEN_TEST_KEY").unwrap();
        assert_eq!(key.as_bytes(), test_key.as_bytes());

        // Cleanup
        unsafe {
            std::env::remove_var("WARDEN_TEST_KEY");
        }
    }
}
