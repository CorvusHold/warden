//! Notification manager for dispatching events to configured channels.

use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

use super::config::{ChannelType, NotificationConfig, NotificationDefaults};
use super::events::Event;
use super::providers::{EmailProvider, NotificationProvider, SlackProvider, WebhookProvider};

/// Result of a notification attempt.
#[derive(Debug, Clone)]
pub struct NotificationResult {
    /// Channel name
    pub channel: String,
    /// Whether the notification was sent successfully
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Number of retry attempts made
    pub attempts: u32,
}

/// Notification manager that handles event dispatch to configured channels.
pub struct NotificationManager {
    config: NotificationConfig,
    providers: Vec<(String, Arc<dyn NotificationProvider>)>,
}

impl NotificationManager {
    /// Create a new notification manager from configuration.
    pub fn new(config: NotificationConfig) -> Self {
        let mut providers: Vec<(String, Arc<dyn NotificationProvider>)> = Vec::new();

        for channel in &config.channels {
            if !channel.enabled {
                continue;
            }

            let provider: Arc<dyn NotificationProvider> = match &channel.channel_type {
                ChannelType::Webhook(webhook_config) => Arc::new(WebhookProvider::new(
                    webhook_config.clone(),
                    channel.name.clone(),
                )),
                ChannelType::Slack(slack_config) => Arc::new(SlackProvider::new(
                    slack_config.clone(),
                    channel.name.clone(),
                )),
                ChannelType::Email(email_config) => Arc::new(EmailProvider::new(
                    email_config.clone(),
                    channel.name.clone(),
                )),
            };

            providers.push((channel.name.clone(), provider));
        }

        Self { config, providers }
    }

    /// Get the notification configuration.
    pub fn config(&self) -> &NotificationConfig {
        &self.config
    }

    /// Check if notifications are configured.
    pub fn has_channels(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Get the number of enabled channels.
    pub fn channel_count(&self) -> usize {
        self.providers.len()
    }

    /// Send a notification to all matching channels.
    ///
    /// This method is non-blocking and will not fail the caller even if
    /// notifications fail to send. Errors are logged but not propagated.
    pub async fn notify(&self, event: &Event) -> Vec<NotificationResult> {
        let mut results = Vec::new();

        // Check if we should send based on defaults
        if !self.should_notify(event) {
            debug!(
                "Skipping notification for event {} based on defaults",
                event.event_type_str()
            );
            return results;
        }

        // Find matching channels
        let matching_channels = self.config.channels_for_event(event.event_type_str());

        if matching_channels.is_empty() {
            debug!(
                "No channels configured for event {}",
                event.event_type_str()
            );
            return results;
        }

        info!(
            "Sending notification for event {} to {} channel(s)",
            event.event_type_str(),
            matching_channels.len()
        );

        // Send to each matching channel
        for channel in matching_channels {
            if let Some((_, provider)) = self.providers.iter().find(|(name, _)| name == &channel.name) {
                let result = self
                    .send_with_retry(provider.clone(), event, &self.config.defaults)
                    .await;
                results.push(result);
            }
        }

        results
    }

    /// Send notification with retry logic.
    async fn send_with_retry(
        &self,
        provider: Arc<dyn NotificationProvider>,
        event: &Event,
        defaults: &NotificationDefaults,
    ) -> NotificationResult {
        let channel_name = provider.name().to_string();
        let max_attempts = defaults.retry_attempts.max(1);
        let retry_delay = Duration::from_secs(defaults.retry_delay_secs);

        for attempt in 1..=max_attempts {
            debug!(
                "Notification attempt {}/{} for channel '{}'",
                attempt, max_attempts, channel_name
            );

            match provider.send(event).await {
                Ok(()) => {
                    info!(
                        "Notification sent successfully to channel '{}' (attempt {})",
                        channel_name, attempt
                    );
                    return NotificationResult {
                        channel: channel_name,
                        success: true,
                        error: None,
                        attempts: attempt,
                    };
                }
                Err(e) => {
                    warn!(
                        "Notification to channel '{}' failed (attempt {}): {}",
                        channel_name, attempt, e
                    );

                    if attempt < max_attempts {
                        debug!("Retrying in {} seconds...", retry_delay.as_secs());
                        sleep(retry_delay).await;
                    } else {
                        error!(
                            "Notification to channel '{}' failed after {} attempts: {}",
                            channel_name, max_attempts, e
                        );
                        return NotificationResult {
                            channel: channel_name,
                            success: false,
                            error: Some(e.to_string()),
                            attempts: attempt,
                        };
                    }
                }
            }
        }

        // Should not reach here, but just in case
        NotificationResult {
            channel: channel_name,
            success: false,
            error: Some("Unknown error".to_string()),
            attempts: max_attempts,
        }
    }

    /// Check if we should send a notification based on defaults.
    fn should_notify(&self, event: &Event) -> bool {
        let defaults = &self.config.defaults;

        if event.event_type.is_failure() {
            return defaults.on_failure;
        }

        if event.event_type.is_success() {
            return defaults.on_success;
        }

        // For other events (started, warning, etc.), always notify
        true
    }

    /// Test a specific channel by sending a test notification.
    pub async fn test_channel(&self, channel_name: &str) -> Result<NotificationResult, String> {
        let (_, provider) = self
            .providers
            .iter()
            .find(|(name, _)| name == channel_name)
            .ok_or_else(|| format!("Channel '{}' not found or not enabled", channel_name))?;

        info!("Testing notification channel '{}'", channel_name);

        match provider.test().await {
            Ok(()) => {
                info!("Test notification sent successfully to channel '{}'", channel_name);
                Ok(NotificationResult {
                    channel: channel_name.to_string(),
                    success: true,
                    error: None,
                    attempts: 1,
                })
            }
            Err(e) => {
                error!("Test notification to channel '{}' failed: {}", channel_name, e);
                Ok(NotificationResult {
                    channel: channel_name.to_string(),
                    success: false,
                    error: Some(e.to_string()),
                    attempts: 1,
                })
            }
        }
    }

    /// List all configured channels.
    pub fn list_channels(&self) -> Vec<ChannelInfo> {
        self.config
            .channels
            .iter()
            .map(|c| ChannelInfo {
                name: c.name.clone(),
                channel_type: match &c.channel_type {
                    ChannelType::Webhook(_) => "webhook".to_string(),
                    ChannelType::Slack(_) => "slack".to_string(),
                    ChannelType::Email(_) => "email".to_string(),
                },
                enabled: c.enabled,
                events: c.events.clone(),
            })
            .collect()
    }
}

/// Information about a notification channel.
#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub events: Vec<String>,
}

/// Background notification dispatcher.
///
/// This provides a fire-and-forget interface for sending notifications
/// without blocking the main operation.
#[allow(dead_code)] // Will be used by daemon for background notification dispatch
pub struct NotificationDispatcher {
    sender: mpsc::Sender<Event>,
}

impl NotificationDispatcher {
    /// Create a new dispatcher with the given manager.
    ///
    /// Spawns a background task to process notifications.
    #[allow(dead_code)] // Will be used by daemon for background notification dispatch
    pub fn new(manager: Arc<NotificationManager>) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Event>(100);

        // Spawn background task to process notifications
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                let results = manager.notify(&event).await;

                // Log summary
                let success_count = results.iter().filter(|r| r.success).count();
                let fail_count = results.len() - success_count;

                if fail_count > 0 {
                    warn!(
                        "Notification dispatch for event {}: {} succeeded, {} failed",
                        event.event_type_str(),
                        success_count,
                        fail_count
                    );
                } else if success_count > 0 {
                    debug!(
                        "Notification dispatch for event {}: {} succeeded",
                        event.event_type_str(),
                        success_count
                    );
                }
            }
        });

        Self { sender }
    }

    /// Queue an event for notification dispatch.
    ///
    /// This method is non-blocking and will not fail even if the queue is full.
    #[allow(dead_code)] // Will be used by daemon for background notification dispatch
    pub fn dispatch(&self, event: Event) {
        if let Err(e) = self.sender.try_send(event) {
            warn!("Failed to queue notification: {}", e);
        }
    }

    /// Queue an event for notification dispatch (async version).
    #[allow(dead_code)] // Will be used by daemon for background notification dispatch
    pub async fn dispatch_async(&self, event: Event) {
        if let Err(e) = self.sender.send(event).await {
            warn!("Failed to queue notification: {}", e);
        }
    }
}

impl Clone for NotificationDispatcher {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::config::*;
    use crate::notifications::events::*;

    fn create_test_config() -> NotificationConfig {
        NotificationConfig {
            channels: vec![
                NotificationChannel {
                    name: "test-webhook".to_string(),
                    channel_type: ChannelType::Webhook(WebhookChannelConfig {
                        url: "http://localhost:9999/webhook".to_string(),
                        headers: Default::default(),
                        timeout_secs: 5,
                        method: "POST".to_string(),
                    }),
                    events: vec!["backup.*".to_string()],
                    enabled: true,
                },
                NotificationChannel {
                    name: "disabled-channel".to_string(),
                    channel_type: ChannelType::Webhook(WebhookChannelConfig {
                        url: "http://localhost:9999/disabled".to_string(),
                        headers: Default::default(),
                        timeout_secs: 5,
                        method: "POST".to_string(),
                    }),
                    events: vec![],
                    enabled: false,
                },
            ],
            defaults: NotificationDefaults {
                on_failure: true,
                on_success: false,
                include_details: true,
                retry_attempts: 2,
                retry_delay_secs: 1,
            },
        }
    }

    #[test]
    fn test_manager_creation() {
        let config = create_test_config();
        let manager = NotificationManager::new(config);

        assert!(manager.has_channels());
        assert_eq!(manager.channel_count(), 1); // Only enabled channels
    }

    #[test]
    fn test_list_channels() {
        let config = create_test_config();
        let manager = NotificationManager::new(config);

        let channels = manager.list_channels();
        assert_eq!(channels.len(), 2); // All channels, including disabled

        let enabled: Vec<_> = channels.iter().filter(|c| c.enabled).collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "test-webhook");
    }

    #[test]
    fn test_should_notify_failure() {
        let config = NotificationConfig {
            channels: vec![],
            defaults: NotificationDefaults {
                on_failure: true,
                on_success: false,
                ..Default::default()
            },
        };
        let manager = NotificationManager::new(config);

        let failure_event = Event::new(EventType::BackupFailed, "Test failure");
        assert!(manager.should_notify(&failure_event));

        let success_event = Event::new(EventType::BackupCompleted, "Test success");
        assert!(!manager.should_notify(&success_event));
    }

    #[test]
    fn test_should_notify_success() {
        let config = NotificationConfig {
            channels: vec![],
            defaults: NotificationDefaults {
                on_failure: true,
                on_success: true,
                ..Default::default()
            },
        };
        let manager = NotificationManager::new(config);

        let success_event = Event::new(EventType::BackupCompleted, "Test success");
        assert!(manager.should_notify(&success_event));
    }
}
