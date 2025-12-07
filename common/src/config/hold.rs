//! HOLD/C2 Integration Configuration
//!
//! This module defines the configuration schema for optional HOLD control plane integration.
//! When disabled (the default), Warden operates in fully standalone mode with no HOLD overhead.

use serde::{Deserialize, Serialize};

/// HOLD integration configuration
///
/// Controls the optional connection to a HOLD/C2 control plane.
/// When `enabled` is false, no HOLD connection is attempted and Warden
/// operates in fully standalone mode.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HoldConfig {
    /// Master switch for HOLD integration (default: false)
    #[serde(default)]
    pub enabled: bool,

    /// AMQP endpoint for HOLD connection (e.g., "amqp://hold.example.com:5672")
    #[serde(default)]
    pub endpoint: Option<String>,

    /// Credentials for HOLD authentication
    #[serde(default)]
    pub credentials: Option<HoldCredentials>,

    /// Agent identifier for HOLD registration
    /// Use "auto" to derive from hostname/MAC, or specify a custom ID
    #[serde(default = "default_agent_id")]
    pub agent_id: String,

    /// Interval in seconds between heartbeat/status publications (default: 60)
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,

    /// Maximum time in seconds to wait for a command to complete (default: 300)
    #[serde(default = "default_command_timeout")]
    pub command_timeout_secs: u64,

    /// Retry configuration for HOLD connection
    #[serde(default)]
    pub retry: HoldRetryConfig,

    /// TLS configuration for secure connections
    #[serde(default)]
    pub tls: Option<HoldTlsConfig>,

    /// Labels to attach to this agent for HOLD filtering/grouping
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
}

fn default_agent_id() -> String {
    "auto".to_string()
}

fn default_heartbeat_interval() -> u64 {
    60
}

fn default_command_timeout() -> u64 {
    300
}

/// Credentials for HOLD authentication
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HoldCredentials {
    /// Username for AMQP authentication
    pub username: Option<String>,

    /// Password for AMQP authentication (plain text - prefer password_env)
    #[serde(skip_serializing)]
    pub password: Option<String>,

    /// Environment variable name containing the password
    pub password_env: Option<String>,

    /// Path to a file containing the password
    pub password_file: Option<String>,
}

impl HoldCredentials {
    /// Resolve the password from the configured source
    ///
    /// Priority: password_env > password_file > password
    pub fn resolve_password(&self) -> Option<String> {
        // Try environment variable first
        if let Some(env_var) = &self.password_env {
            if let Ok(password) = std::env::var(env_var) {
                return Some(password);
            }
        }

        // Try password file
        if let Some(file_path) = &self.password_file {
            if let Ok(password) = std::fs::read_to_string(file_path) {
                return Some(password.trim().to_string());
            }
        }

        // Fall back to plain password
        self.password.clone()
    }
}

/// Retry configuration for HOLD connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldRetryConfig {
    /// Maximum number of connection attempts before giving up (default: 3)
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,

    /// Initial backoff duration in seconds (default: 5)
    #[serde(default = "default_backoff_secs")]
    pub backoff_secs: u64,

    /// Maximum backoff duration in seconds (default: 300)
    #[serde(default = "default_max_backoff_secs")]
    pub max_backoff_secs: u64,

    /// Backoff multiplier for exponential backoff (default: 2.0)
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,
}

fn default_max_attempts() -> u32 {
    3
}

fn default_backoff_secs() -> u64 {
    5
}

fn default_max_backoff_secs() -> u64 {
    300
}

fn default_backoff_multiplier() -> f64 {
    2.0
}

impl Default for HoldRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            backoff_secs: default_backoff_secs(),
            max_backoff_secs: default_max_backoff_secs(),
            backoff_multiplier: default_backoff_multiplier(),
        }
    }
}

/// TLS configuration for secure HOLD connections
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HoldTlsConfig {
    /// Enable TLS (default: true for amqps:// endpoints)
    #[serde(default = "default_tls_enabled")]
    pub enabled: bool,

    /// Skip certificate verification (NOT recommended for production)
    #[serde(default)]
    pub skip_verify: bool,

    /// Path to CA certificate file
    pub ca_cert_path: Option<String>,

    /// Path to client certificate file (for mTLS)
    pub client_cert_path: Option<String>,

    /// Path to client key file (for mTLS)
    pub client_key_path: Option<String>,
}

fn default_tls_enabled() -> bool {
    true
}

impl HoldConfig {
    /// Check if HOLD integration is enabled and properly configured
    pub fn is_configured(&self) -> bool {
        self.enabled && self.endpoint.is_some()
    }

    /// Get the resolved agent ID
    ///
    /// If agent_id is "auto", derives an ID from the hostname.
    /// Otherwise returns the configured agent_id.
    pub fn resolve_agent_id(&self) -> String {
        if self.agent_id == "auto" {
            // Derive from hostname
            match hostname::get() {
                Ok(hostname) => format!("warden-{}", hostname.to_string_lossy()),
                Err(_) => format!("warden-{}", uuid::Uuid::new_v4()),
            }
        } else {
            self.agent_id.clone()
        }
    }

    /// Get the AMQP connection URI with credentials
    pub fn get_connection_uri(&self) -> Option<String> {
        let endpoint = self.endpoint.as_ref()?;

        // Parse the endpoint to inject credentials if needed
        if let Some(creds) = &self.credentials {
            let username = creds.username.as_deref().unwrap_or("guest");
            let password = creds.resolve_password().unwrap_or_default();

            // Check if endpoint already has credentials
            if endpoint.contains('@') {
                return Some(endpoint.clone());
            }

            // Inject credentials into the URI
            if let Some(rest) = endpoint.strip_prefix("amqp://") {
                return Some(format!("amqp://{}:{}@{}", username, password, rest));
            }
            if let Some(rest) = endpoint.strip_prefix("amqps://") {
                return Some(format!("amqps://{}:{}@{}", username, password, rest));
            }
        }

        Some(endpoint.clone())
    }
}

/// Integration configuration section
///
/// Contains all optional integration configurations (HOLD, future integrations)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationConfig {
    /// HOLD/C2 control plane integration
    #[serde(default)]
    pub hold: HoldConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hold_config_defaults() {
        let config = HoldConfig::default();
        assert!(!config.enabled);
        assert!(config.endpoint.is_none());
        assert_eq!(config.agent_id, "auto");
        assert_eq!(config.heartbeat_interval_secs, 60);
        assert_eq!(config.command_timeout_secs, 300);
    }

    #[test]
    fn test_hold_config_is_configured() {
        let mut config = HoldConfig::default();
        assert!(!config.is_configured());

        config.enabled = true;
        assert!(!config.is_configured()); // Still missing endpoint

        config.endpoint = Some("amqp://localhost:5672".to_string());
        assert!(config.is_configured());
    }

    #[test]
    fn test_resolve_agent_id_auto() {
        let config = HoldConfig {
            agent_id: "auto".to_string(),
            ..Default::default()
        };
        let resolved = config.resolve_agent_id();
        assert!(resolved.starts_with("warden-"));
    }

    #[test]
    fn test_resolve_agent_id_custom() {
        let config = HoldConfig {
            agent_id: "my-custom-agent".to_string(),
            ..Default::default()
        };
        assert_eq!(config.resolve_agent_id(), "my-custom-agent");
    }

    #[test]
    fn test_get_connection_uri_with_credentials() {
        let config = HoldConfig {
            endpoint: Some("amqp://localhost:5672".to_string()),
            credentials: Some(HoldCredentials {
                username: Some("user".to_string()),
                password: Some("pass".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let uri = config.get_connection_uri().unwrap();
        assert_eq!(uri, "amqp://user:pass@localhost:5672");
    }

    #[test]
    fn test_retry_config_defaults() {
        let config = HoldRetryConfig::default();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.backoff_secs, 5);
        assert_eq!(config.max_backoff_secs, 300);
        assert_eq!(config.backoff_multiplier, 2.0);
    }
}
