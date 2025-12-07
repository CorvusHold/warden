//! Notification providers for different channel types.

use async_trait::async_trait;
use log::{debug, error};
use serde::Serialize;
use std::time::Duration;
use thiserror::Error;

use super::config::{EmailChannelConfig, SlackChannelConfig, WebhookChannelConfig};
use super::events::Event;

/// Errors that can occur when sending notifications.
#[derive(Error, Debug)]
pub enum NotificationError {
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("SMTP error: {0}")]
    SmtpError(String),

    #[error("Timeout after {0} seconds")]
    Timeout(u64),

    #[error("Provider not available: {0}")]
    NotAvailable(String),
}

/// Result type for notification operations.
pub type NotificationResult<T> = Result<T, NotificationError>;

/// Trait for notification providers.
#[async_trait]
pub trait NotificationProvider: Send + Sync {
    /// Send a notification for the given event.
    async fn send(&self, event: &Event) -> NotificationResult<()>;

    /// Get the provider name for logging.
    fn name(&self) -> &str;

    /// Test the provider configuration by sending a test notification.
    async fn test(&self) -> NotificationResult<()>;
}

/// Webhook notification provider.
pub struct WebhookProvider {
    config: WebhookChannelConfig,
    channel_name: String,
}

impl WebhookProvider {
    pub fn new(config: WebhookChannelConfig, channel_name: String) -> Self {
        Self {
            config,
            channel_name,
        }
    }
}

#[async_trait]
impl NotificationProvider for WebhookProvider {
    async fn send(&self, event: &Event) -> NotificationResult<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .build()
            .map_err(|e| NotificationError::HttpError(e.to_string()))?;

        let payload = serde_json::to_string(event)
            .map_err(|e| NotificationError::SerializationError(e.to_string()))?;

        debug!(
            "Sending webhook notification to {} for event {}",
            self.config.url,
            event.event_type_str()
        );

        let mut request = match self.config.method.to_uppercase().as_str() {
            "POST" => client.post(&self.config.url),
            "PUT" => client.put(&self.config.url),
            _ => client.post(&self.config.url),
        };

        // Add custom headers
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        // Set content type if not already set
        if !self.config.headers.contains_key("Content-Type") {
            request = request.header("Content-Type", "application/json");
        }

        let response = request
            .body(payload)
            .send()
            .await
            .map_err(|e| NotificationError::HttpError(e.to_string()))?;

        if response.status().is_success() {
            debug!("Webhook notification sent successfully");
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(NotificationError::HttpError(format!(
                "HTTP {}: {}",
                status, body
            )))
        }
    }

    fn name(&self) -> &str {
        &self.channel_name
    }

    async fn test(&self) -> NotificationResult<()> {
        let test_event = Event::new(
            super::events::EventType::StatusWarning,
            "Test notification from Warden",
        )
        .with_label("test", "true");

        self.send(&test_event).await
    }
}

/// Slack notification provider.
pub struct SlackProvider {
    config: SlackChannelConfig,
    channel_name: String,
}

impl SlackProvider {
    pub fn new(config: SlackChannelConfig, channel_name: String) -> Self {
        Self {
            config,
            channel_name,
        }
    }

    fn format_slack_message(&self, event: &Event) -> SlackMessage {
        let color = match event.severity {
            super::events::EventSeverity::Info => "#36a64f",    // Green
            super::events::EventSeverity::Warning => "#ffcc00", // Yellow
            super::events::EventSeverity::Critical => "#ff0000", // Red
        };

        let mut fields = vec![
            SlackField {
                title: "Event Type".to_string(),
                value: event.event_type_str().to_string(),
                short: true,
            },
            SlackField {
                title: "Severity".to_string(),
                value: event.severity.to_string(),
                short: true,
            },
        ];

        if let Some(ref hostname) = event.hostname {
            fields.push(SlackField {
                title: "Host".to_string(),
                value: hostname.clone(),
                short: true,
            });
        }

        // Add payload-specific fields
        if let Some(ref payload) = event.payload {
            match payload {
                super::events::EventPayload::Backup(bp) => {
                    if let Some(ref db) = bp.database {
                        fields.push(SlackField {
                            title: "Database".to_string(),
                            value: db.clone(),
                            short: true,
                        });
                    }
                    if let Some(ref backup_id) = bp.backup_id {
                        fields.push(SlackField {
                            title: "Backup ID".to_string(),
                            value: backup_id.clone(),
                            short: true,
                        });
                    }
                    if let Some(size) = bp.size_bytes {
                        fields.push(SlackField {
                            title: "Size".to_string(),
                            value: format_bytes(size),
                            short: true,
                        });
                    }
                    if let Some(duration) = bp.duration_secs {
                        fields.push(SlackField {
                            title: "Duration".to_string(),
                            value: format!("{:.1}s", duration),
                            short: true,
                        });
                    }
                    if let Some(ref error) = bp.error {
                        fields.push(SlackField {
                            title: "Error".to_string(),
                            value: error.clone(),
                            short: false,
                        });
                    }
                }
                super::events::EventPayload::Ha(hp) => {
                    if let Some(ref cluster) = hp.cluster_id {
                        fields.push(SlackField {
                            title: "Cluster".to_string(),
                            value: cluster.clone(),
                            short: true,
                        });
                    }
                    if let Some(ref from) = hp.from_node {
                        fields.push(SlackField {
                            title: "From Node".to_string(),
                            value: from.clone(),
                            short: true,
                        });
                    }
                    if let Some(ref to) = hp.to_node {
                        fields.push(SlackField {
                            title: "To Node".to_string(),
                            value: to.clone(),
                            short: true,
                        });
                    }
                    if let Some(ref error) = hp.error {
                        fields.push(SlackField {
                            title: "Error".to_string(),
                            value: error.clone(),
                            short: false,
                        });
                    }
                }
                _ => {}
            }
        }

        let attachment = SlackAttachment {
            color: color.to_string(),
            title: format!("Warden: {}", event.event_type_str()),
            text: event.message.clone(),
            fields,
            ts: event.timestamp.timestamp(),
        };

        SlackMessage {
            channel: self.config.channel.clone(),
            username: self.config.username.clone(),
            icon_emoji: self.config.icon_emoji.clone(),
            attachments: vec![attachment],
        }
    }
}

#[async_trait]
impl NotificationProvider for SlackProvider {
    async fn send(&self, event: &Event) -> NotificationResult<()> {
        let webhook_url = self
            .config
            .resolve_webhook_url()
            .ok_or_else(|| NotificationError::ConfigError("Slack webhook URL not configured".to_string()))?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .build()
            .map_err(|e| NotificationError::HttpError(e.to_string()))?;

        let message = self.format_slack_message(event);
        let payload = serde_json::to_string(&message)
            .map_err(|e| NotificationError::SerializationError(e.to_string()))?;

        debug!(
            "Sending Slack notification for event {}",
            event.event_type_str()
        );

        let response = client
            .post(&webhook_url)
            .header("Content-Type", "application/json")
            .body(payload)
            .send()
            .await
            .map_err(|e| NotificationError::HttpError(e.to_string()))?;

        if response.status().is_success() {
            debug!("Slack notification sent successfully");
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(NotificationError::HttpError(format!(
                "HTTP {}: {}",
                status, body
            )))
        }
    }

    fn name(&self) -> &str {
        &self.channel_name
    }

    async fn test(&self) -> NotificationResult<()> {
        let test_event = Event::new(
            super::events::EventType::StatusWarning,
            "Test notification from Warden - your notification channel is configured correctly!",
        )
        .with_label("test", "true");

        self.send(&test_event).await
    }
}

/// Slack message format.
#[derive(Debug, Serialize)]
struct SlackMessage {
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon_emoji: Option<String>,
    attachments: Vec<SlackAttachment>,
}

#[derive(Debug, Serialize)]
struct SlackAttachment {
    color: String,
    title: String,
    text: String,
    fields: Vec<SlackField>,
    ts: i64,
}

#[derive(Debug, Serialize)]
struct SlackField {
    title: String,
    value: String,
    short: bool,
}

/// Email notification provider.
pub struct EmailProvider {
    config: EmailChannelConfig,
    channel_name: String,
}

impl EmailProvider {
    pub fn new(config: EmailChannelConfig, channel_name: String) -> Self {
        Self {
            config,
            channel_name,
        }
    }

    fn format_subject(&self, event: &Event) -> String {
        let prefix = self
            .config
            .subject_prefix
            .as_deref()
            .unwrap_or("[Warden]");
        let severity_indicator = match event.severity {
            super::events::EventSeverity::Info => "ℹ️",
            super::events::EventSeverity::Warning => "⚠️",
            super::events::EventSeverity::Critical => "🚨",
        };
        format!(
            "{} {} {}",
            prefix,
            severity_indicator,
            event.event_type_str()
        )
    }

    fn format_body(&self, event: &Event) -> String {
        let mut body = String::new();

        body.push_str(&format!("Event: {}\n", event.event_type_str()));
        body.push_str(&format!("Severity: {}\n", event.severity));
        body.push_str(&format!("Time: {}\n", event.timestamp.format("%Y-%m-%d %H:%M:%S UTC")));

        if let Some(ref hostname) = event.hostname {
            body.push_str(&format!("Host: {}\n", hostname));
        }

        body.push_str(&format!("\nMessage:\n{}\n", event.message));

        // Add payload details
        if let Some(ref payload) = event.payload {
            body.push_str("\nDetails:\n");
            match payload {
                super::events::EventPayload::Backup(bp) => {
                    if let Some(ref id) = bp.backup_id {
                        body.push_str(&format!("  Backup ID: {}\n", id));
                    }
                    if let Some(ref db) = bp.database {
                        body.push_str(&format!("  Database: {}\n", db));
                    }
                    if let Some(ref host) = bp.host {
                        body.push_str(&format!("  Host: {}\n", host));
                    }
                    if let Some(ref bt) = bp.backup_type {
                        body.push_str(&format!("  Backup Type: {}\n", bt));
                    }
                    if let Some(size) = bp.size_bytes {
                        body.push_str(&format!("  Size: {}\n", format_bytes(size)));
                    }
                    if let Some(duration) = bp.duration_secs {
                        body.push_str(&format!("  Duration: {:.1}s\n", duration));
                    }
                    if let Some(ref path) = bp.local_path {
                        body.push_str(&format!("  Local Path: {}\n", path));
                    }
                    if let Some(ref path) = bp.remote_path {
                        body.push_str(&format!("  Remote Path: {}\n", path));
                    }
                    if let Some(ref error) = bp.error {
                        body.push_str(&format!("  Error: {}\n", error));
                    }
                }
                super::events::EventPayload::Restore(rp) => {
                    if let Some(ref id) = rp.backup_id {
                        body.push_str(&format!("  Backup ID: {}\n", id));
                    }
                    if let Some(ref dir) = rp.target_dir {
                        body.push_str(&format!("  Target Dir: {}\n", dir));
                    }
                    if let Some(duration) = rp.duration_secs {
                        body.push_str(&format!("  Duration: {:.1}s\n", duration));
                    }
                    if let Some(ref error) = rp.error {
                        body.push_str(&format!("  Error: {}\n", error));
                    }
                }
                super::events::EventPayload::Pitr(pp) => {
                    if let Some(ref id) = pp.backup_id {
                        body.push_str(&format!("  Base Backup: {}\n", id));
                    }
                    if let Some(ref time) = pp.target_time {
                        body.push_str(&format!("  Target Time: {}\n", time));
                    }
                    if let Some(segments) = pp.wal_segments_applied {
                        body.push_str(&format!("  WAL Segments: {}\n", segments));
                    }
                    if let Some(ref start) = pp.gap_start {
                        body.push_str(&format!("  Gap Start: {}\n", start));
                    }
                    if let Some(ref end) = pp.gap_end {
                        body.push_str(&format!("  Gap End: {}\n", end));
                    }
                    if let Some(ref error) = pp.error {
                        body.push_str(&format!("  Error: {}\n", error));
                    }
                }
                super::events::EventPayload::Retention(rp) => {
                    if let Some(evaluated) = rp.backups_evaluated {
                        body.push_str(&format!("  Backups Evaluated: {}\n", evaluated));
                    }
                    if let Some(deleted) = rp.backups_deleted {
                        body.push_str(&format!("  Backups Deleted: {}\n", deleted));
                    }
                    if let Some(space) = rp.space_reclaimed_bytes {
                        body.push_str(&format!("  Space Reclaimed: {}\n", format_bytes(space)));
                    }
                    if let Some(ref error) = rp.error {
                        body.push_str(&format!("  Error: {}\n", error));
                    }
                }
                super::events::EventPayload::Ha(hp) => {
                    if let Some(ref cluster) = hp.cluster_id {
                        body.push_str(&format!("  Cluster: {}\n", cluster));
                    }
                    if let Some(ref op) = hp.operation {
                        body.push_str(&format!("  Operation: {}\n", op));
                    }
                    if let Some(ref from) = hp.from_node {
                        body.push_str(&format!("  From Node: {}\n", from));
                    }
                    if let Some(ref to) = hp.to_node {
                        body.push_str(&format!("  To Node: {}\n", to));
                    }
                    if let Some(ref loss) = hp.data_loss_estimate {
                        body.push_str(&format!("  Data Loss Estimate: {}\n", loss));
                    }
                    if let Some(ref error) = hp.error {
                        body.push_str(&format!("  Error: {}\n", error));
                    }
                }
                super::events::EventPayload::Status(sp) => {
                    if let Some(ref component) = sp.component {
                        body.push_str(&format!("  Component: {}\n", component));
                    }
                    if let Some(ref msg) = sp.message {
                        body.push_str(&format!("  Status: {}\n", msg));
                    }
                }
                super::events::EventPayload::Generic(map) => {
                    for (key, value) in map {
                        body.push_str(&format!("  {}: {}\n", key, value));
                    }
                }
            }
        }

        if !event.labels.is_empty() {
            body.push_str("\nLabels:\n");
            for (key, value) in &event.labels {
                body.push_str(&format!("  {}: {}\n", key, value));
            }
        }

        body.push_str(&format!("\n---\nEvent ID: {}\n", event.id));

        body
    }
}

#[async_trait]
impl NotificationProvider for EmailProvider {
    async fn send(&self, event: &Event) -> NotificationResult<()> {
        // Note: Full SMTP implementation would require the `lettre` crate.
        // This implementation returns an error to indicate email is not configured,
        // preventing silent failures where notifications appear to succeed but don't send.

        let subject = self.format_subject(event);
        let _body = self.format_body(event);

        debug!(
            "Email notification prepared for event {}: subject='{}', to={:?}",
            event.event_type_str(),
            subject,
            self.config.to
        );

        // Return an error since SMTP is not implemented.
        // This prevents callers from treating the notification as successfully sent.
        // To enable email notifications, add the 'lettre' crate and implement SMTP support.
        Err(NotificationError::NotAvailable(
            "Email notifications require SMTP configuration. \
             Add 'lettre' crate and configure SMTP settings to enable email sending.".to_string()
        ))
    }

    fn name(&self) -> &str {
        &self.channel_name
    }

    async fn test(&self) -> NotificationResult<()> {
        let test_event = Event::new(
            super::events::EventType::StatusWarning,
            "Test notification from Warden - your email notification channel is configured correctly!",
        )
        .with_label("test", "true");

        self.send(&test_event).await
    }
}

/// Format bytes into human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_slack_message_formatting() {
        let config = SlackChannelConfig {
            webhook_url: Some("https://hooks.slack.com/test".to_string()),
            webhook_url_env: None,
            channel: Some("#alerts".to_string()),
            username: Some("Warden".to_string()),
            icon_emoji: Some(":shield:".to_string()),
            timeout_secs: 30,
        };

        let provider = SlackProvider::new(config, "test-slack".to_string());

        let event = super::super::events::BackupEventBuilder::completed()
            .backup_id("backup-123")
            .database("mydb")
            .size_bytes(1024 * 1024 * 100)
            .duration_secs(45.5)
            .build();

        let message = provider.format_slack_message(&event);

        assert_eq!(message.channel, Some("#alerts".to_string()));
        assert_eq!(message.username, Some("Warden".to_string()));
        assert!(!message.attachments.is_empty());
        assert_eq!(message.attachments[0].color, "#36a64f"); // Green for info
    }

    #[test]
    fn test_email_subject_formatting() {
        let config = EmailChannelConfig {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            smtp_user: None,
            smtp_password: None,
            smtp_password_env: None,
            use_tls: true,
            from: "warden@example.com".to_string(),
            to: vec!["admin@example.com".to_string()],
            cc: vec![],
            subject_prefix: Some("[PROD-WARDEN]".to_string()),
            timeout_secs: 30,
        };

        let provider = EmailProvider::new(config, "test-email".to_string());

        let event = Event::new(
            super::super::events::EventType::BackupFailed,
            "Backup failed due to connection timeout",
        );

        let subject = provider.format_subject(&event);
        assert!(subject.contains("[PROD-WARDEN]"));
        assert!(subject.contains("🚨")); // Critical emoji
        assert!(subject.contains("backup.failed"));
    }
}
