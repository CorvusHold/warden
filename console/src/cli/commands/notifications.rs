//! CLI commands for notification management.

use clap::{Args, Subcommand};
use common::config::load_config;
use common::notifications::NotificationManager;
use log::error;

/// Notification management commands
#[derive(Args, Debug)]
pub struct Notifications {
    #[clap(subcommand)]
    pub command: NotificationCommands,
}

#[derive(Subcommand, Debug)]
pub enum NotificationCommands {
    /// List configured notification channels
    ///
    /// Shows all notification channels configured in the Warden config file,
    /// including their type, enabled status, and subscribed events.
    List {
        /// Output format (table or json)
        #[clap(long, short, default_value = "table")]
        format: String,
    },

    /// Test a notification channel
    ///
    /// Sends a test notification to verify the channel is configured correctly.
    /// This helps validate webhook URLs, Slack integrations, and email settings.
    Test {
        /// Name of the channel to test
        #[clap(long, short)]
        channel: String,
    },

    /// Validate notification configuration
    ///
    /// Checks the notification configuration for errors without sending any notifications.
    Validate,

    /// Show notification configuration details
    ///
    /// Displays the full notification configuration including defaults and channel details.
    Show {
        /// Output format (table or json)
        #[clap(long, short, default_value = "table")]
        format: String,
    },
}

impl Notifications {
    pub async fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            NotificationCommands::List { format } => {
                list_channels(format)?;
            }
            NotificationCommands::Test { channel } => {
                test_channel(channel).await?;
            }
            NotificationCommands::Validate => {
                validate_config()?;
            }
            NotificationCommands::Show { format } => {
                show_config(format)?;
            }
        }
        Ok(())
    }
}

/// List all configured notification channels.
fn list_channels(format: &str) -> anyhow::Result<()> {
    let config = load_config()?;
    let notification_config = &config.notifications;

    if format == "json" {
        let channels: Vec<_> = notification_config
            .channels
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "type": match &c.channel_type {
                        common::notifications::ChannelType::Webhook(_) => "webhook",
                        common::notifications::ChannelType::Slack(_) => "slack",
                        common::notifications::ChannelType::Email(_) => "email",
                    },
                    "enabled": c.enabled,
                    "events": c.events,
                })
            })
            .collect();

        println!("{}", serde_json::to_string_pretty(&channels)?);
    } else {
        if notification_config.channels.is_empty() {
            println!("No notification channels configured.");
            println!();
            println!(
                "To configure notifications, add a 'notifications' section to your warden.toml:"
            );
            println!();
            println!("  [notifications]");
            println!("  [[notifications.channels]]");
            println!("  name = \"ops-webhook\"");
            println!("  type = \"webhook\"");
            println!("  url = \"https://hooks.example.com/warden\"");
            println!("  events = [\"backup.failed\", \"ha.*\"]");
            return Ok(());
        }

        println!("Notification Channels");
        println!("=====================");
        println!();

        for channel in &notification_config.channels {
            let channel_type = match &channel.channel_type {
                common::notifications::ChannelType::Webhook(_) => "webhook",
                common::notifications::ChannelType::Slack(_) => "slack",
                common::notifications::ChannelType::Email(_) => "email",
            };

            let status = if channel.enabled {
                "✓ enabled"
            } else {
                "✗ disabled"
            };

            println!("  {} [{}] {}", channel.name, channel_type, status);

            if !channel.events.is_empty() {
                println!("    Events: {}", channel.events.join(", "));
            } else {
                println!("    Events: (all)");
            }
            println!();
        }

        println!("Total: {} channel(s)", notification_config.channels.len());
    }

    Ok(())
}

/// Test a specific notification channel.
async fn test_channel(channel_name: &str) -> anyhow::Result<()> {
    let config = load_config()?;
    let notification_config = config.notifications;

    // Check if channel exists
    if notification_config.get_channel(channel_name).is_none() {
        error!("Channel '{}' not found in configuration", channel_name);
        println!("Error: Channel '{}' not found.", channel_name);
        println!();
        println!("Available channels:");
        for c in &notification_config.channels {
            println!("  - {}", c.name);
        }
        anyhow::bail!("Channel '{}' not found", channel_name);
    }

    println!("Testing notification channel '{}'...", channel_name);

    let manager = NotificationManager::new(notification_config);

    match manager.test_channel(channel_name).await {
        Ok(result) => {
            if result.success {
                println!("✓ Test notification sent successfully!");
                println!();
                println!("The channel '{}' is configured correctly.", channel_name);
            } else {
                println!("✗ Test notification failed.");
                if let Some(error) = result.error {
                    println!("  Error: {}", error);
                }
                println!();
                println!("Please check your channel configuration and try again.");
            }
        }
        Err(e) => {
            println!("✗ Failed to test channel: {}", e);
        }
    }

    Ok(())
}

/// Validate the notification configuration.
fn validate_config() -> anyhow::Result<()> {
    let config = load_config()?;
    let notification_config = &config.notifications;

    println!("Validating notification configuration...");
    println!();

    match notification_config.validate() {
        Ok(()) => {
            println!("✓ Configuration is valid.");
            println!();
            println!("Summary:");
            println!(
                "  - {} channel(s) configured",
                notification_config.channels.len()
            );

            let enabled_count = notification_config
                .channels
                .iter()
                .filter(|c| c.enabled)
                .count();
            println!("  - {} channel(s) enabled", enabled_count);

            println!();
            println!("Default settings:");
            println!(
                "  - Notify on failure: {}",
                notification_config.defaults.on_failure
            );
            println!(
                "  - Notify on success: {}",
                notification_config.defaults.on_success
            );
            println!(
                "  - Include details: {}",
                notification_config.defaults.include_details
            );
            println!(
                "  - Retry attempts: {}",
                notification_config.defaults.retry_attempts
            );
            println!(
                "  - Retry delay: {}s",
                notification_config.defaults.retry_delay_secs
            );
        }
        Err(errors) => {
            println!("✗ Configuration has {} error(s):", errors.len());
            println!();
            for error in errors {
                println!("  - {}", error);
            }
            anyhow::bail!("Notification configuration is invalid");
        }
    }

    Ok(())
}

/// Show the full notification configuration.
fn show_config(format: &str) -> anyhow::Result<()> {
    let config = load_config()?;
    let notification_config = &config.notifications;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&notification_config)?);
    } else {
        println!("Notification Configuration");
        println!("==========================");
        println!();

        println!("Defaults:");
        println!(
            "  Notify on failure: {}",
            notification_config.defaults.on_failure
        );
        println!(
            "  Notify on success: {}",
            notification_config.defaults.on_success
        );
        println!(
            "  Include details: {}",
            notification_config.defaults.include_details
        );
        println!(
            "  Retry attempts: {}",
            notification_config.defaults.retry_attempts
        );
        println!(
            "  Retry delay: {}s",
            notification_config.defaults.retry_delay_secs
        );
        println!();

        if notification_config.channels.is_empty() {
            println!("Channels: (none configured)");
        } else {
            println!("Channels:");
            for channel in &notification_config.channels {
                println!();
                println!("  [{}]", channel.name);
                println!("    Enabled: {}", channel.enabled);

                match &channel.channel_type {
                    common::notifications::ChannelType::Webhook(wh) => {
                        println!("    Type: webhook");
                        println!("    URL: {}", wh.url);
                        println!("    Method: {}", wh.method);
                        println!("    Timeout: {}s", wh.timeout_secs);
                        if !wh.headers.is_empty() {
                            println!("    Headers: {:?}", wh.headers.keys().collect::<Vec<_>>());
                        }
                    }
                    common::notifications::ChannelType::Slack(sl) => {
                        println!("    Type: slack");
                        if sl.webhook_url.is_some() {
                            println!("    Webhook URL: (configured)");
                        } else if sl.webhook_url_env.is_some() {
                            println!(
                                "    Webhook URL: env:{}",
                                sl.webhook_url_env.as_ref().unwrap()
                            );
                        }
                        if let Some(ref ch) = sl.channel {
                            println!("    Channel: {}", ch);
                        }
                        if let Some(ref user) = sl.username {
                            println!("    Username: {}", user);
                        }
                        println!("    Timeout: {}s", sl.timeout_secs);
                    }
                    common::notifications::ChannelType::Email(em) => {
                        println!("    Type: email");
                        println!("    SMTP Host: {}", em.smtp_host);
                        println!("    SMTP Port: {}", em.smtp_port);
                        println!("    From: {}", em.from);
                        println!("    To: {}", em.to.join(", "));
                        if !em.cc.is_empty() {
                            println!("    CC: {}", em.cc.join(", "));
                        }
                        println!("    TLS: {}", em.use_tls);
                    }
                }

                if channel.events.is_empty() {
                    println!("    Events: (all)");
                } else {
                    println!("    Events: {}", channel.events.join(", "));
                }
            }
        }
    }

    Ok(())
}
