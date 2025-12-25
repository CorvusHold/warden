//! Encryption configuration structures.

use serde::{Deserialize, Serialize};

/// Supported encryption algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM (default, recommended)
    #[default]
    #[serde(alias = "aes256-gcm", alias = "AES256-GCM")]
    Aes256Gcm,
}

impl EncryptionAlgorithm {
    /// Get the algorithm identifier byte for the file format.
    pub fn format_id(&self) -> u8 {
        match self {
            EncryptionAlgorithm::Aes256Gcm => 0x01,
        }
    }

    /// Parse algorithm from format ID byte.
    pub fn from_format_id(id: u8) -> Option<Self> {
        match id {
            0x01 => Some(EncryptionAlgorithm::Aes256Gcm),
            _ => None,
        }
    }

    /// Get the key size in bytes for this algorithm.
    pub fn key_size(&self) -> usize {
        match self {
            EncryptionAlgorithm::Aes256Gcm => 32, // 256 bits
        }
    }

    /// Get the nonce size in bytes for this algorithm.
    pub fn nonce_size(&self) -> usize {
        match self {
            EncryptionAlgorithm::Aes256Gcm => 12, // 96 bits
        }
    }

    /// Get the authentication tag size in bytes.
    pub fn tag_size(&self) -> usize {
        match self {
            EncryptionAlgorithm::Aes256Gcm => 16, // 128 bits
        }
    }
}

impl std::fmt::Display for EncryptionAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncryptionAlgorithm::Aes256Gcm => write!(f, "aes256-gcm"),
        }
    }
}

/// Encryption configuration for backups and WAL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Whether encryption is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Key reference (env:VAR_NAME or file:/path/to/key).
    #[serde(default)]
    pub key_ref: Option<String>,

    /// Encryption algorithm (defaults to aes256-gcm).
    #[serde(default)]
    pub algorithm: EncryptionAlgorithm,

    /// Whether to encrypt metadata files.
    #[serde(default)]
    pub encrypt_metadata: bool,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            key_ref: None,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            encrypt_metadata: false,
        }
    }
}

impl EncryptionConfig {
    /// Create a new encryption config with encryption enabled.
    pub fn enabled(key_ref: impl Into<String>) -> Self {
        Self {
            enabled: true,
            key_ref: Some(key_ref.into()),
            algorithm: EncryptionAlgorithm::Aes256Gcm,
            encrypt_metadata: false,
        }
    }

    /// Create a disabled encryption config.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Check if encryption is properly configured.
    pub fn is_configured(&self) -> bool {
        self.enabled && self.key_ref.is_some()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.key_ref.is_none() {
            return Err("Encryption is enabled but no key_ref is configured".to_string());
        }
        Ok(())
    }

    /// Get a redacted string representation for logging.
    pub fn redacted_string(&self) -> String {
        if !self.enabled {
            return "encryption: disabled".to_string();
        }

        let key_display = match &self.key_ref {
            Some(k) if k.starts_with("env:") => k.clone(),
            Some(k) if k.starts_with("file:") => k.clone(),
            Some(_) => "[REDACTED]".to_string(),
            None => "[NOT_SET]".to_string(),
        };

        format!(
            "encryption: enabled, algorithm: {}, key: {}",
            self.algorithm, key_display
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_format_id_roundtrip() {
        let algo = EncryptionAlgorithm::Aes256Gcm;
        let id = algo.format_id();
        let parsed = EncryptionAlgorithm::from_format_id(id);
        assert_eq!(parsed, Some(algo));
    }

    #[test]
    fn test_algorithm_sizes() {
        let algo = EncryptionAlgorithm::Aes256Gcm;
        assert_eq!(algo.key_size(), 32);
        assert_eq!(algo.nonce_size(), 12);
        assert_eq!(algo.tag_size(), 16);
    }

    #[test]
    fn test_config_validation() {
        let mut config = EncryptionConfig::default();
        assert!(config.validate().is_ok());

        config.enabled = true;
        assert!(config.validate().is_err());

        config.key_ref = Some("env:MY_KEY".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_redacted_string() {
        let config = EncryptionConfig::enabled("env:MY_KEY");
        let redacted = config.redacted_string();
        assert!(redacted.contains("env:MY_KEY"));
        assert!(!redacted.contains("REDACTED"));

        let config2 = EncryptionConfig::enabled("plain_key_value");
        let redacted2 = config2.redacted_string();
        assert!(redacted2.contains("REDACTED"));
        assert!(!redacted2.contains("plain_key_value"));
    }

    #[test]
    fn test_config_serde() {
        let yaml = r#"
enabled: true
key_ref: "env:BACKUP_KEY"
algorithm: aes256-gcm
encrypt_metadata: false
"#;
        let config: EncryptionConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.key_ref, Some("env:BACKUP_KEY".to_string()));
        assert_eq!(config.algorithm, EncryptionAlgorithm::Aes256Gcm);
    }
}
