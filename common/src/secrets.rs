//! Secret reference handling and resolution.
//!
//! This module provides utilities for handling secrets that may be stored
//! as environment variable references (`env:VAR_NAME`) or file references
//! (`file:/path/to/secret`).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Errors that can occur when resolving secrets.
#[derive(Debug, Error)]
pub enum SecretError {
    #[error("Environment variable not set: {0}")]
    EnvVarNotSet(String),

    #[error("Cannot read secret file '{path}': {reason}")]
    FileReadError { path: String, reason: String },

    #[error("Invalid secret reference format: {0}")]
    InvalidFormat(String),

    #[error("Secret file has insecure permissions: {path} (expected 0600, got {mode:o})")]
    InsecurePermissions { path: String, mode: u32 },

    #[error("Empty secret value")]
    EmptySecret,
}

/// A reference to a secret value that can be resolved at runtime.
///
/// Supports two formats:
/// - `env:VAR_NAME` - Read from environment variable
/// - `file:/path/to/secret` - Read from file
/// - Plain value (not recommended for production)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    /// Create a new secret reference.
    pub fn new(reference: impl Into<String>) -> Self {
        Self(reference.into())
    }

    /// Create an environment variable reference.
    pub fn from_env(var_name: impl Into<String>) -> Self {
        Self(format!("env:{}", var_name.into()))
    }

    /// Create a file reference.
    pub fn from_file(path: impl Into<String>) -> Self {
        Self(format!("file:{}", path.into()))
    }

    /// Get the raw reference string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check if this is an environment variable reference.
    pub fn is_env_ref(&self) -> bool {
        self.0.starts_with("env:")
    }

    /// Check if this is a file reference.
    pub fn is_file_ref(&self) -> bool {
        self.0.starts_with("file:")
    }

    /// Check if this is a plain value (not a reference).
    pub fn is_plain_value(&self) -> bool {
        !self.is_env_ref() && !self.is_file_ref()
    }

    /// Resolve the secret reference to its actual value.
    ///
    /// # Security
    /// The returned `ResolvedSecret` will zeroize its contents when dropped.
    pub fn resolve(&self) -> Result<ResolvedSecret, SecretError> {
        if let Some(var_name) = self.0.strip_prefix("env:") {
            resolve_env_secret(var_name)
        } else if let Some(path) = self.0.strip_prefix("file:") {
            resolve_file_secret(path)
        } else {
            // Plain value - return as-is (with warning in logs)
            log::warn!(
                "Secret is stored as plain value (not recommended). Use env: or file: prefix."
            );
            if self.0.is_empty() {
                return Err(SecretError::EmptySecret);
            }
            Ok(ResolvedSecret(self.0.clone()))
        }
    }

    /// Get a redacted display string for logging.
    pub fn redacted(&self) -> RedactedSecretRef<'_> {
        RedactedSecretRef(self)
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Always show the reference type, never the actual value
        if self.is_env_ref() || self.is_file_ref() {
            write!(f, "{}", self.0)
        } else {
            write!(f, "[REDACTED]")
        }
    }
}

/// A wrapper for displaying secret references in logs without exposing values.
pub struct RedactedSecretRef<'a>(&'a SecretRef);

impl<'a> fmt::Display for RedactedSecretRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_env_ref() || self.0.is_file_ref() {
            write!(f, "{}", self.0 .0)
        } else {
            write!(f, "[REDACTED_VALUE]")
        }
    }
}

impl fmt::Debug for RedactedSecretRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretRef({})", self)
    }
}

/// A resolved secret value that is zeroized when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct ResolvedSecret(String);

impl ResolvedSecret {
    /// Create a new resolved secret from a string.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Get the secret value as a string slice.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Get the secret value as bytes.
    pub fn expose_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Consume the secret and return the inner value.
    ///
    /// # Security
    /// The caller is responsible for zeroizing the returned value.
    pub fn into_inner(self) -> String {
        // Note: This bypasses zeroize-on-drop, caller must handle cleanup
        // Clone before self is dropped to preserve the value
        self.0.clone()
    }
}

impl fmt::Debug for ResolvedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResolvedSecret([REDACTED])")
    }
}

impl fmt::Display for ResolvedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

/// Resolve a secret from an environment variable.
fn resolve_env_secret(var_name: &str) -> Result<ResolvedSecret, SecretError> {
    match std::env::var(var_name) {
        Ok(value) if value.is_empty() => Err(SecretError::EmptySecret),
        Ok(value) => Ok(ResolvedSecret(value)),
        Err(_) => Err(SecretError::EnvVarNotSet(var_name.to_string())),
    }
}

/// Resolve a secret from a file.
fn resolve_file_secret(path: &str) -> Result<ResolvedSecret, SecretError> {
    let expanded_path = shellexpand::full(path)
        .map_err(|e| SecretError::FileReadError {
            path: path.to_string(),
            reason: e.to_string(),
        })?
        .into_owned();

    let path_obj = Path::new(&expanded_path);

    // Check file permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path_obj) {
            let mode = metadata.permissions().mode() & 0o777;
            // Warn if permissions are too open (should be 0600 or 0400)
            if mode & 0o077 != 0 {
                log::warn!(
                    "Secret file {} has overly permissive permissions ({:o}). Consider chmod 0600.",
                    expanded_path,
                    mode
                );
            }
        }
    }

    let content = fs::read_to_string(path_obj).map_err(|e| SecretError::FileReadError {
        path: expanded_path.clone(),
        reason: e.to_string(),
    })?;

    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        return Err(SecretError::EmptySecret);
    }

    Ok(ResolvedSecret(trimmed))
}

/// Resolve an optional secret reference.
///
/// This is a convenience function for resolving `Option<String>` values
/// that may contain secret references.
pub fn resolve_optional_secret(value: &Option<String>) -> Option<String> {
    value.as_ref().and_then(|v| {
        if let Some(env_var) = v.strip_prefix("env:") {
            std::env::var(env_var).ok()
        } else if let Some(path) = v.strip_prefix("file:") {
            let expanded = shellexpand::full(path).ok()?.into_owned();
            fs::read_to_string(&expanded)
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            Some(v.clone())
        }
    })
}

/// Redact a string value for safe logging.
///
/// Returns the original value if it's a reference (env: or file:),
/// otherwise returns "[REDACTED]".
pub fn redact_for_logging(value: &str) -> &str {
    if value.starts_with("env:") || value.starts_with("file:") {
        value
    } else {
        "[REDACTED]"
    }
}

/// Redact an optional string value for safe logging.
pub fn redact_optional_for_logging(value: &Option<String>) -> String {
    match value {
        Some(v) => redact_for_logging(v).to_string(),
        None => "[NOT_SET]".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_ref_env() {
        let secret = SecretRef::from_env("TEST_VAR");
        assert!(secret.is_env_ref());
        assert!(!secret.is_file_ref());
        assert!(!secret.is_plain_value());
        assert_eq!(secret.as_str(), "env:TEST_VAR");
    }

    #[test]
    fn test_secret_ref_file() {
        let secret = SecretRef::from_file("/etc/secret");
        assert!(!secret.is_env_ref());
        assert!(secret.is_file_ref());
        assert!(!secret.is_plain_value());
        assert_eq!(secret.as_str(), "file:/etc/secret");
    }

    #[test]
    fn test_secret_ref_plain() {
        let secret = SecretRef::new("plain_value");
        assert!(!secret.is_env_ref());
        assert!(!secret.is_file_ref());
        assert!(secret.is_plain_value());
    }

    #[test]
    fn test_resolve_env_secret() {
        // Set test env var
        unsafe {
            std::env::set_var("WARDEN_TEST_SECRET", "test_value_123");
        }

        let secret = SecretRef::from_env("WARDEN_TEST_SECRET");
        let resolved = secret.resolve().unwrap();
        assert_eq!(resolved.expose(), "test_value_123");

        // Cleanup
        unsafe {
            std::env::remove_var("WARDEN_TEST_SECRET");
        }
    }

    #[test]
    fn test_resolve_missing_env() {
        let secret = SecretRef::from_env("NONEXISTENT_VAR_12345");
        let result = secret.resolve();
        assert!(matches!(result, Err(SecretError::EnvVarNotSet(_))));
    }

    #[test]
    fn test_resolved_secret_debug_redacted() {
        let secret = ResolvedSecret::new("super_secret_value");
        let debug_str = format!("{:?}", secret);
        assert!(!debug_str.contains("super_secret_value"));
        assert!(debug_str.contains("REDACTED"));
    }

    #[test]
    fn test_resolved_secret_display_redacted() {
        let secret = ResolvedSecret::new("super_secret_value");
        let display_str = format!("{}", secret);
        assert!(!display_str.contains("super_secret_value"));
        assert!(display_str.contains("REDACTED"));
    }

    #[test]
    fn test_secret_ref_display() {
        let env_ref = SecretRef::from_env("MY_VAR");
        assert_eq!(format!("{}", env_ref), "env:MY_VAR");

        let file_ref = SecretRef::from_file("/path/to/key");
        assert_eq!(format!("{}", file_ref), "file:/path/to/key");

        let plain = SecretRef::new("plain_value");
        assert_eq!(format!("{}", plain), "[REDACTED]");
    }

    #[test]
    fn test_redact_for_logging() {
        assert_eq!(redact_for_logging("env:MY_VAR"), "env:MY_VAR");
        assert_eq!(redact_for_logging("file:/path"), "file:/path");
        assert_eq!(redact_for_logging("plain_secret"), "[REDACTED]");
    }

    #[test]
    fn test_resolve_optional_secret() {
        // Set test env var
        unsafe {
            std::env::set_var("WARDEN_OPT_TEST", "opt_value");
        }

        let env_ref = Some("env:WARDEN_OPT_TEST".to_string());
        assert_eq!(
            resolve_optional_secret(&env_ref),
            Some("opt_value".to_string())
        );

        let plain = Some("plain".to_string());
        assert_eq!(resolve_optional_secret(&plain), Some("plain".to_string()));

        let none: Option<String> = None;
        assert_eq!(resolve_optional_secret(&none), None);

        // Cleanup
        unsafe {
            std::env::remove_var("WARDEN_OPT_TEST");
        }
    }
}
