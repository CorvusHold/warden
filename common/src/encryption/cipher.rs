//! AES-256-GCM encryption and decryption.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use super::config::EncryptionAlgorithm;
use super::format::{EncryptedFileHeader, HEADER_SIZE};
use super::keys::EncryptionKey;
use super::EncryptionError;

/// Encrypt data in memory.
///
/// Returns the encrypted data with header prepended.
pub fn encrypt_data(
    plaintext: &[u8],
    key: &EncryptionKey,
    algorithm: EncryptionAlgorithm,
) -> Result<Vec<u8>, EncryptionError> {
    match algorithm {
        EncryptionAlgorithm::Aes256Gcm => encrypt_aes256_gcm(plaintext, key),
    }
}

/// Decrypt data in memory.
///
/// Expects data with header prepended.
pub fn decrypt_data(ciphertext: &[u8], key: &EncryptionKey) -> Result<Vec<u8>, EncryptionError> {
    // Parse header
    let header = EncryptedFileHeader::from_bytes(ciphertext)?;

    match header.algorithm {
        EncryptionAlgorithm::Aes256Gcm => decrypt_aes256_gcm(ciphertext, key, &header),
    }
}

/// Encrypt a file.
///
/// Reads the source file, encrypts it, and writes to the destination.
pub fn encrypt_file(
    source: &Path,
    dest: &Path,
    key: &EncryptionKey,
    algorithm: EncryptionAlgorithm,
) -> Result<(), EncryptionError> {
    log::debug!("Encrypting file: {:?} -> {:?}", source, dest);

    // Read source file
    let plaintext = fs::read(source)?;

    // Encrypt
    let encrypted = encrypt_data(&plaintext, key, algorithm)?;

    // Write to destination
    let mut file = fs::File::create(dest)?;
    file.write_all(&encrypted)?;
    file.sync_all()?;

    log::debug!(
        "Encrypted {} bytes -> {} bytes",
        plaintext.len(),
        encrypted.len()
    );

    Ok(())
}

/// Decrypt a file.
///
/// Reads the encrypted source file, decrypts it, and writes to the destination.
pub fn decrypt_file(
    source: &Path,
    dest: &Path,
    key: &EncryptionKey,
) -> Result<(), EncryptionError> {
    log::debug!("Decrypting file: {:?} -> {:?}", source, dest);

    // Read encrypted file
    let ciphertext = fs::read(source)?;

    // Decrypt
    let plaintext = decrypt_data(&ciphertext, key)?;

    // Write to destination
    let mut file = fs::File::create(dest)?;
    file.write_all(&plaintext)?;
    file.sync_all()?;

    log::debug!(
        "Decrypted {} bytes -> {} bytes",
        ciphertext.len(),
        plaintext.len()
    );

    Ok(())
}

/// Encrypt data using AES-256-GCM.
fn encrypt_aes256_gcm(plaintext: &[u8], key: &EncryptionKey) -> Result<Vec<u8>, EncryptionError> {
    // Create header with random nonce
    let header = EncryptedFileHeader::new(EncryptionAlgorithm::Aes256Gcm);

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|e| {
        EncryptionError::EncryptionFailed(format!("Failed to create cipher: {}", e))
    })?;

    // Create nonce
    let nonce = Nonce::from_slice(&header.nonce);

    // Encrypt (ciphertext includes auth tag)
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| EncryptionError::EncryptionFailed(format!("Encryption failed: {}", e)))?;

    // Combine header and ciphertext
    let mut result = header.to_bytes();
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data using AES-256-GCM.
fn decrypt_aes256_gcm(
    data: &[u8],
    key: &EncryptionKey,
    header: &EncryptedFileHeader,
) -> Result<Vec<u8>, EncryptionError> {
    if data.len() <= HEADER_SIZE {
        return Err(EncryptionError::InvalidFormat(
            "Encrypted data too short".to_string(),
        ));
    }

    // Extract ciphertext (after header)
    let ciphertext = &data[HEADER_SIZE..];

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).map_err(|e| {
        EncryptionError::DecryptionFailed(format!("Failed to create cipher: {}", e))
    })?;

    // Create nonce from header
    let nonce = Nonce::from_slice(&header.nonce);

    // Decrypt and verify
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncryptionError::AuthenticationFailed)?;

    Ok(plaintext)
}

/// Encrypt data in-place for streaming (returns header + ciphertext).
///
/// This is useful for encrypting data before uploading to S3.
#[allow(dead_code)] // Will be used for S3 upload encryption
pub fn encrypt_streaming(
    mut reader: impl Read,
    key: &EncryptionKey,
    algorithm: EncryptionAlgorithm,
) -> Result<Vec<u8>, EncryptionError> {
    let mut plaintext = Vec::new();
    reader
        .read_to_end(&mut plaintext)
        .map_err(EncryptionError::IoError)?;

    encrypt_data(&plaintext, key, algorithm)
}

/// Decrypt data from a reader.
#[allow(dead_code)] // Will be used for S3 download decryption
pub fn decrypt_streaming(
    mut reader: impl Read,
    key: &EncryptionKey,
) -> Result<Vec<u8>, EncryptionError> {
    let mut ciphertext = Vec::new();
    reader
        .read_to_end(&mut ciphertext)
        .map_err(EncryptionError::IoError)?;

    decrypt_data(&ciphertext, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_key() -> EncryptionKey {
        EncryptionKey::generate_random()
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = test_key();
        let plaintext = b"Hello, World! This is a test message.";

        let encrypted = encrypt_data(plaintext, &key, EncryptionAlgorithm::Aes256Gcm).unwrap();

        // Encrypted data should be larger (header + tag)
        assert!(encrypted.len() > plaintext.len());

        // Should start with magic
        assert_eq!(&encrypted[0..8], b"WARDEN01");

        let decrypted = decrypt_data(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = test_key();
        let key2 = test_key();
        let plaintext = b"Secret data";

        let encrypted = encrypt_data(plaintext, &key1, EncryptionAlgorithm::Aes256Gcm).unwrap();

        let result = decrypt_data(&encrypted, &key2);
        assert!(matches!(result, Err(EncryptionError::AuthenticationFailed)));
    }

    #[test]
    fn test_corrupted_data_fails() {
        let key = test_key();
        let plaintext = b"Secret data";

        let mut encrypted = encrypt_data(plaintext, &key, EncryptionAlgorithm::Aes256Gcm).unwrap();

        // Corrupt the ciphertext
        if let Some(byte) = encrypted.last_mut() {
            *byte ^= 0xFF;
        }

        let result = decrypt_data(&encrypted, &key);
        assert!(matches!(result, Err(EncryptionError::AuthenticationFailed)));
    }

    #[test]
    fn test_empty_data() {
        let key = test_key();
        let plaintext = b"";

        let encrypted = encrypt_data(plaintext, &key, EncryptionAlgorithm::Aes256Gcm).unwrap();
        let decrypted = decrypt_data(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_large_data() {
        let key = test_key();
        let plaintext: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();

        let encrypted = encrypt_data(&plaintext, &key, EncryptionAlgorithm::Aes256Gcm).unwrap();
        let decrypted = decrypt_data(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_file_encrypt_decrypt() {
        let key = test_key();
        let dir = tempdir().unwrap();

        let source = dir.path().join("source.txt");
        let encrypted = dir.path().join("encrypted.bin");
        let decrypted = dir.path().join("decrypted.txt");

        // Write source file
        fs::write(&source, b"File content to encrypt").unwrap();

        // Encrypt
        encrypt_file(&source, &encrypted, &key, EncryptionAlgorithm::Aes256Gcm).unwrap();

        // Verify encrypted file has header
        let enc_data = fs::read(&encrypted).unwrap();
        assert_eq!(&enc_data[0..8], b"WARDEN01");

        // Decrypt
        decrypt_file(&encrypted, &decrypted, &key).unwrap();

        // Verify content
        let content = fs::read(&decrypted).unwrap();
        assert_eq!(content, b"File content to encrypt");
    }

    #[test]
    fn test_streaming_encrypt_decrypt() {
        let key = test_key();
        let plaintext = b"Streaming test data";

        let encrypted =
            encrypt_streaming(&plaintext[..], &key, EncryptionAlgorithm::Aes256Gcm).unwrap();
        let decrypted = decrypt_streaming(&encrypted[..], &key).unwrap();

        assert_eq!(decrypted, plaintext);
    }
}
