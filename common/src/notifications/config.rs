//! Notification configuration types.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Top-level notification configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Notification channels
    #[serde(default)]
    pub channels: Vec<NotificationChannel>,

    /// Default notification settings
    #[serde(default)]
    pub defaults: NotificationDefaults,
}

/// Default notification settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationDefaults {
    /// Send notifications on failure events
    #[serde(default = "default_true")]
    pub on_failure: bool,

    /// Send notifications on success events
    #[serde(default)]
    pub on_success: bool,

    /// Include detailed information in notifications
    #[serde(default = "default_true")]
    pub include_details: bool,

    /// Number of retry attempts for failed notifications
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,

    /// Delay between retry attempts in seconds
    #[serde(default = "default_retry_delay_secs")]
    pub retry_delay_secs: u64,
}

impl Default for NotificationDefaults {
    fn default() -> Self {
        Self {
            on_failure: true,
            on_success: false,
            include_details: true,
            retry_attempts: 3,
            retry_delay_secs: 5,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_retry_attempts() -> u32 {
    3
}

fn default_retry_delay_secs() -> u64 {
    5
}

/// A notification channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannel {
    /// Unique name for this channel
    pub name: String,

    /// Channel type and configuration
    #[serde(flatten)]
    pub channel_type: ChannelType,

    /// Event patterns to subscribe to (e.g., "backup.failed", "ha.*")
    #[serde(default)]
    pub events: Vec<String>,

    /// Whether this channel is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl NotificationChannel {
    /// Check if this channel should receive the given event type.
    pub fn matches_event(&self, event_type: &str) -> bool {
        if self.events.is_empty() {
            return true; // No filter means all events
        }

        for pattern in &self.events {
            if Self::matches_pattern(pattern, event_type) {
                return true;
            }
        }
        false
    }

    /// Match an event type against a pattern.
    /// Supports wildcards: "backup.*" matches "backup.started", "backup.failed", etc.
    fn matches_pattern(pattern: &str, event_type: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if let Some(prefix) = pattern.strip_suffix(".*") {
            return event_type.starts_with(prefix)
                && event_type.len() > prefix.len()
                && event_type.chars().nth(prefix.len()) == Some('.');
        }

        pattern == event_type
    }
}

/// Channel type with type-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelType {
    /// Webhook notification channel
    Webhook(WebhookChannelConfig),

    /// Slack notification channel
    Slack(SlackChannelConfig),

    /// Email notification channel
    Email(EmailChannelConfig),
}

/// Webhook channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookChannelConfig {
    /// Webhook URL to POST notifications to
    pub url: String,

    /// Optional HTTP headers to include
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// HTTP method (default: POST)
    #[serde(default = "default_method")]
    pub method: String,
}

fn default_timeout() -> u64 {
    30
}

fn default_method() -> String {
    "POST".to_string()
}

/// Slack channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackChannelConfig {
    /// Slack webhook URL (can be a direct URL or env var reference like "env:SLACK_WEBHOOK_URL")
    #[serde(default)]
    pub webhook_url: Option<String>,

    /// Environment variable containing the webhook URL
    #[serde(default)]
    pub webhook_url_env: Option<String>,

    /// Slack channel to post to (optional, uses webhook default if not set)
    #[serde(default)]
    pub channel: Option<String>,

    /// Username to display (optional)
    #[serde(default)]
    pub username: Option<String>,

    /// Emoji icon to display (optional, e.g., ":warning:")
    #[serde(default)]
    pub icon_emoji: Option<String>,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl SlackChannelConfig {
    /// Resolve the webhook URL from config or environment.
    pub fn resolve_webhook_url(&self) -> Option<String> {
        // First try direct URL
        if let Some(ref url) = self.webhook_url {
            if let Some(env_var) = url.strip_prefix("env:") {
                return std::env::var(env_var).ok();
            }
            return Some(url.clone());
        }

        // Then try env var reference
        if let Some(ref env_var) = self.webhook_url_env {
            return std::env::var(env_var).ok();
        }

        None
    }
}

/// Email channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailChannelConfig {
    /// SMTP server hostname
    pub smtp_host: String,

    /// SMTP server port
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,

    /// SMTP username (optional)
    #[serde(default)]
    pub smtp_user: Option<String>,

    /// SMTP password (can be env var reference like "env:SMTP_PASSWORD")
    #[serde(default)]
    pub smtp_password: Option<String>,

    /// Environment variable containing SMTP password
    #[serde(default)]
    pub smtp_password_env: Option<String>,

    /// Use TLS/STARTTLS
    #[serde(default = "default_true")]
    pub use_tls: bool,

    /// From email address
    pub from: String,

    /// To email addresses
    pub to: Vec<String>,

    /// CC email addresses (optional)
    #[serde(default)]
    pub cc: Vec<String>,

    /// Email subject prefix (optional)
    #[serde(default)]
    pub subject_prefix: Option<String>,

    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_smtp_port() -> u16 {
    587
}

impl EmailChannelConfig {
    /// Resolve the SMTP password from config or environment.
    pub fn resolve_smtp_password(&self) -> Option<String> {
        // First try direct password
        if let Some(ref password) = self.smtp_password {
            if let Some(env_var) = password.strip_prefix("env:") {
                return std::env::var(env_var).ok();
            }
            return Some(password.clone());
        }

        // Then try env var reference
        if let Some(ref env_var) = self.smtp_password_env {
            return std::env::var(env_var).ok();
        }

        None
    }
}

impl NotificationConfig {
    /// Get all enabled channels.
    pub fn enabled_channels(&self) -> impl Iterator<Item = &NotificationChannel> {
        self.channels.iter().filter(|c| c.enabled)
    }

    /// Get channels that should receive the given event type.
    pub fn channels_for_event(&self, event_type: &str) -> Vec<&NotificationChannel> {
        self.enabled_channels()
            .filter(|c| c.matches_event(event_type))
            .collect()
    }

    /// Get a channel by name.
    pub fn get_channel(&self, name: &str) -> Option<&NotificationChannel> {
        self.channels.iter().find(|c| c.name == name)
    }

    /// Validate the notification configuration.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // Check for duplicate channel names
        let mut seen_names = HashSet::new();
        for channel in &self.channels {
            if !seen_names.insert(&channel.name) {
                errors.push(format!("Duplicate channel name: {}", channel.name));
            }

            // Validate channel-specific config
            match &channel.channel_type {
                ChannelType::Webhook(config) => {
                    if config.url.is_empty() {
                        errors.push(format!("Channel '{}': webhook URL is required", channel.name));
                    }
                }
                ChannelType::Slack(config) => {
                    if config.webhook_url.is_none() && config.webhook_url_env.is_none() {
                        errors.push(format!(
                            "Channel '{}': Slack webhook_url or webhook_url_env is required",
                            channel.name
                        ));
                    }
                }
                ChannelType::Email(config) => {
                    if config.smtp_host.is_empty() {
                        errors.push(format!(
                            "Channel '{}': SMTP host is required",
                            channel.name
                        ));
                    }
                    if config.from.is_empty() {
                        errors.push(format!(
                            "Channel '{}': from address is required",
                            channel.name
                        ));
                    }
                    if config.to.is_empty() {
                        errors.push(format!(
                            "Channel '{}': at least one recipient is required",
                            channel.name
                        ));
                    }
                }
            }

            // Validate event patterns
            for pattern in &channel.events {
                if pattern.is_empty() {
                    errors.push(format!(
                        "Channel '{}': empty event pattern",
                        channel.name
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_pattern_matching() {
        let channel = NotificationChannel {
            name: "test".to_string(),
            channel_type: ChannelType::Webhook(WebhookChannelConfig {
                url: "http://example.com".to_string(),
                headers: Default::default(),
                timeout_secs: 30,
                method: "POST".to_string(),
            }),
            events: vec![
                "backup.failed".to_string(),
                "ha.*".to_string(),
            ],
            enabled: true,
        };

        // Exact match
        assert!(channel.matches_event("backup.failed"));
        assert!(!channel.matches_event("backup.started"));

        // Wildcard match
        assert!(channel.matches_event("ha.failover"));
        assert!(channel.matches_event("ha.switchover.started"));
        assert!(!channel.matches_event("ha")); // Must have something after the dot

        // No match
        assert!(!channel.matches_event("restore.failed"));
    }

    #[test]
    fn test_empty_events_matches_all() {
        let channel = NotificationChannel {
            name: "test".to_string(),
            channel_type: ChannelType::Webhook(WebhookChannelConfig {
                url: "http://example.com".to_string(),
                headers: Default::default(),
                timeout_secs: 30,
                method: "POST".to_string(),
            }),
            events: vec![],
            enabled: true,
        };

        assert!(channel.matches_event("backup.failed"));
        assert!(channel.matches_event("any.event"));
    }

    #[test]
    fn test_slack_webhook_url_resolution() {
        // Direct URL
        let config = SlackChannelConfig {
            webhook_url: Some("https://hooks.slack.com/test".to_string()),
            webhook_url_env: None,
            channel: None,
            username: None,
            icon_emoji: None,
            timeout_secs: 30,
        };
        assert_eq!(
            config.resolve_webhook_url(),
            Some("https://hooks.slack.com/test".to_string())
        );

        // Env var reference in webhook_url
        std::env::set_var("TEST_SLACK_URL", "https://hooks.slack.com/from_env");
        let config = SlackChannelConfig {
            webhook_url: Some("env:TEST_SLACK_URL".to_string()),
            webhook_url_env: None,
            channel: None,
            username: None,
            icon_emoji: None,
            timeout_secs: 30,
        };
        assert_eq!(
            config.resolve_webhook_url(),
            Some("https://hooks.slack.com/from_env".to_string())
        );
        std::env::remove_var("TEST_SLACK_URL");
    }

    #[test]
    fn test_config_validation() {
        let config = NotificationConfig {
            channels: vec![
                NotificationChannel {
                    name: "valid-webhook".to_string(),
                    channel_type: ChannelType::Webhook(WebhookChannelConfig {
                        url: "http://example.com".to_string(),
                        headers: Default::default(),
                        timeout_secs: 30,
                        method: "POST".to_string(),
                    }),
                    events: vec!["backup.*".to_string()],
                    enabled: true,
                },
            ],
            defaults: NotificationDefaults::default(),
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_duplicate_names() {
        let config = NotificationConfig {
            channels: vec![
                NotificationChannel {
                    name: "duplicate".to_string(),
                    channel_type: ChannelType::Webhook(WebhookChannelConfig {
                        url: "http://example.com".to_string(),
                        headers: Default::default(),
                        timeout_secs: 30,
                        method: "POST".to_string(),
                    }),
                    events: vec![],
                    enabled: true,
                },
                NotificationChannel {
                    name: "duplicate".to_string(),
                    channel_type: ChannelType::Webhook(WebhookChannelConfig {
                        url: "http://example2.com".to_string(),
                        headers: Default::default(),
                        timeout_secs: 30,
                        method: "POST".to_string(),
                    }),
                    events: vec![],
                    enabled: true,
                },
            ],
            defaults: NotificationDefaults::default(),
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].contains("Duplicate channel name"));
    }
}
