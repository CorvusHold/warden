//! HOLD Integration Manager
//!
//! Orchestrates the HOLD integration lifecycle including:
//! - Connection management
//! - Command consumption and handling
//! - Periodic heartbeat publishing
//! - Graceful degradation when HOLD is unreachable

use anyhow::Result;
use common::config::HoldConfig;
use futures_util::StreamExt;
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

use super::client::{ConnectionState, HoldClient};
use super::commands::{parse_command, HoldCommandHandler};
use super::events::HoldEventPublisher;

/// State of the HOLD integration
#[derive(Debug, Clone, PartialEq)]
pub enum IntegrationState {
    /// Integration is disabled
    Disabled,
    /// Integration is starting up
    Starting,
    /// Integration is running
    Running,
    /// Integration is stopping
    Stopping,
    /// Integration has stopped
    Stopped,
    /// Integration encountered an error but continues local operation
    Degraded { reason: String },
}

/// HOLD Integration Manager
///
/// Manages the optional connection to a HOLD control plane.
/// When HOLD is unreachable, local operations continue unaffected.
pub struct HoldIntegration {
    config: HoldConfig,
    client: Arc<HoldClient>,
    state: Arc<RwLock<IntegrationState>>,
    command_handler: Arc<HoldCommandHandler>,
    event_publisher: Arc<HoldEventPublisher>,
    tasks: Vec<JoinHandle<()>>,
}

impl HoldIntegration {
    /// Create a new HOLD integration
    pub fn new(config: HoldConfig) -> Self {
        let client = Arc::new(HoldClient::new(config.clone()));
        let agent_id = client.agent_id().to_string();
        let command_handler = Arc::new(HoldCommandHandler::new(agent_id.clone()));
        let event_publisher = Arc::new(HoldEventPublisher::new(client.clone(), agent_id));

        Self {
            config,
            client,
            state: Arc::new(RwLock::new(IntegrationState::Disabled)),
            command_handler,
            event_publisher,
            tasks: Vec::new(),
        }
    }

    /// Check if HOLD integration is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.config.endpoint.is_some()
    }

    /// Get the current integration state
    pub async fn state(&self) -> IntegrationState {
        self.state.read().await.clone()
    }

    /// Start the HOLD integration
    ///
    /// This method is non-blocking and spawns background tasks for:
    /// - Connection management
    /// - Command consumption
    /// - Heartbeat publishing
    ///
    /// If HOLD is unreachable, this logs a warning but does not fail.
    pub async fn start(&mut self) -> Result<()> {
        if !self.is_enabled() {
            info!("HOLD integration disabled");
            *self.state.write().await = IntegrationState::Disabled;
            return Ok(());
        }

        info!("Starting HOLD integration");
        *self.state.write().await = IntegrationState::Starting;

        // Attempt initial connection
        match self.client.connect().await {
            Ok(()) => {
                *self.state.write().await = IntegrationState::Running;
                info!("HOLD integration started successfully");
            }
            Err(e) => {
                warn!("HOLD connection failed, entering degraded mode: {}", e);
                *self.state.write().await = IntegrationState::Degraded {
                    reason: format!("{}", e),
                };
                // Continue - we'll retry in the background
            }
        }

        // Spawn heartbeat task
        let heartbeat_task = self.spawn_heartbeat_task();
        self.tasks.push(heartbeat_task);

        // Spawn command consumer task
        let command_task = self.spawn_command_task();
        self.tasks.push(command_task);

        // Spawn reconnection task
        let reconnect_task = self.spawn_reconnect_task();
        self.tasks.push(reconnect_task);

        Ok(())
    }

    /// Stop the HOLD integration gracefully
    pub async fn stop(&mut self) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        info!("Stopping HOLD integration");
        *self.state.write().await = IntegrationState::Stopping;

        // Cancel all tasks
        for task in self.tasks.drain(..) {
            task.abort();
        }

        // Disconnect from HOLD
        if let Err(e) = self.client.disconnect().await {
            warn!("Error disconnecting from HOLD: {}", e);
        }

        *self.state.write().await = IntegrationState::Stopped;
        info!("HOLD integration stopped");
        Ok(())
    }

    /// Spawn the heartbeat publishing task
    fn spawn_heartbeat_task(&self) -> JoinHandle<()> {
        let publisher = self.event_publisher.clone();
        let interval_secs = self.config.heartbeat_interval_secs;
        let state = self.state.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

            loop {
                interval.tick().await;

                let current_state = state.read().await.clone();
                match current_state {
                    IntegrationState::Running | IntegrationState::Degraded { .. } => {
                        publisher.publish_heartbeat().await;
                    }
                    IntegrationState::Stopping | IntegrationState::Stopped => {
                        debug!("Heartbeat task stopping");
                        break;
                    }
                    _ => {
                        // Skip heartbeat if not in a running state
                    }
                }
            }
        })
    }

    /// Spawn the command consumer task
    fn spawn_command_task(&self) -> JoinHandle<()> {
        let client = self.client.clone();
        let handler = self.command_handler.clone();
        let state = self.state.clone();

        tokio::spawn(async move {
            loop {
                let current_state = state.read().await.clone();
                match current_state {
                    IntegrationState::Stopping | IntegrationState::Stopped => {
                        debug!("Command consumer task stopping");
                        break;
                    }
                    _ => {}
                }

                // Try to get a consumer
                let consumer = match client.consume_commands().await {
                    Ok(Some(c)) => c,
                    Ok(None) => {
                        // Not connected, wait and retry
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    Err(e) => {
                        warn!("Failed to create command consumer: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };

                // Process messages
                let mut consumer = consumer;
                while let Some(delivery_result) = consumer.next().await {
                    match delivery_result {
                        Ok(delivery) => {
                            let payload = &delivery.data;

                            match parse_command(payload) {
                                Ok(envelope) => {
                                    let response = handler.handle(envelope).await;

                                    // Publish response
                                    if let Ok(response_json) = serde_json::to_vec(&response) {
                                        let routing_key = format!(
                                            "warden.responses.hold.{}",
                                            client.agent_id()
                                        );
                                        if let Err(e) = client.publish(&routing_key, &response_json).await {
                                            warn!("Failed to publish command response: {}", e);
                                        }
                                    }

                                    // Acknowledge the message
                                    if let Err(e) = client.ack(delivery.delivery_tag).await {
                                        warn!("Failed to ack message: {}", e);
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to parse HOLD command: {}", e);
                                    // Ack anyway to avoid redelivery of malformed messages
                                    let _ = client.ack(delivery.delivery_tag).await;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Error receiving HOLD command: {}", e);
                            break; // Consumer is broken, will reconnect
                        }
                    }
                }
            }
        })
    }

    /// Spawn the reconnection task
    fn spawn_reconnect_task(&self) -> JoinHandle<()> {
        let client = self.client.clone();
        let state = self.state.clone();
        let retry_config = self.config.retry.clone();

        tokio::spawn(async move {
            let mut backoff = retry_config.backoff_secs;

            loop {
                tokio::time::sleep(Duration::from_secs(backoff)).await;

                let current_state = state.read().await.clone();
                match current_state {
                    IntegrationState::Stopping | IntegrationState::Stopped => {
                        debug!("Reconnect task stopping");
                        break;
                    }
                    IntegrationState::Degraded { .. } => {
                        // Try to reconnect
                        match client.state().await {
                            ConnectionState::Connected => {
                                *state.write().await = IntegrationState::Running;
                                backoff = retry_config.backoff_secs;
                            }
                            _ => {
                                info!("Attempting to reconnect to HOLD...");
                                match client.connect().await {
                                    Ok(()) => {
                                        *state.write().await = IntegrationState::Running;
                                        info!("Reconnected to HOLD");
                                        backoff = retry_config.backoff_secs;
                                    }
                                    Err(e) => {
                                        debug!("Reconnection failed: {}", e);
                                        backoff = (backoff as f64 * retry_config.backoff_multiplier) as u64;
                                        backoff = backoff.min(retry_config.max_backoff_secs);
                                    }
                                }
                            }
                        }
                    }
                    IntegrationState::Running => {
                        // Check if still connected
                        if !client.is_connected().await {
                            *state.write().await = IntegrationState::Degraded {
                                reason: "Connection lost".to_string(),
                            };
                        }
                        backoff = retry_config.backoff_secs;
                    }
                    _ => {
                        // Nothing to do
                    }
                }
            }
        })
    }

    /// Get the event publisher for external use
    pub fn event_publisher(&self) -> Arc<HoldEventPublisher> {
        self.event_publisher.clone()
    }
}

impl Drop for HoldIntegration {
    fn drop(&mut self) {
        // Abort all tasks on drop
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }
}
