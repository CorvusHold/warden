//! Notification system for alerting operators about Warden events.
//!
//! This module provides:
//! - Configuration types for notification channels (webhook, Slack, email)
//! - Standard event types for backup, restore, PITR, HA, and retention operations
//! - Notification providers with async dispatch and retry logic
//! - Non-blocking notification delivery that never fails the main operation

mod config;
mod events;
mod manager;
mod providers;

pub use config::{
    ChannelType, EmailChannelConfig, NotificationChannel, NotificationConfig, NotificationDefaults,
    SlackChannelConfig, WebhookChannelConfig,
};
pub use events::{Event, EventCategory, EventPayload, EventSeverity, EventType};
pub use manager::{NotificationManager, NotificationResult};
pub use providers::{EmailProvider, NotificationProvider, SlackProvider, WebhookProvider};
