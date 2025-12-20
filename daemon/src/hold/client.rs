//! HOLD AMQP Client
//!
//! Manages the AMQP connection to HOLD with automatic reconnection
//! and graceful degradation when HOLD is unreachable.

use anyhow::{Context, Result};
use common::config::HoldConfig;
use lapin::{
    options::*, publisher_confirm::Confirmation, types::FieldTable, BasicProperties, Channel,
    Connection, ConnectionProperties, ExchangeKind,
};
use log::{debug, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Connection state for the HOLD client
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed { reason: String, retry_count: u32 },
}

/// HOLD AMQP client with automatic reconnection
pub struct HoldClient {
    config: HoldConfig,
    connection: Arc<RwLock<Option<Connection>>>,
    channel: Arc<RwLock<Option<Channel>>>,
    state: Arc<RwLock<ConnectionState>>,
    agent_id: String,
}

impl HoldClient {
    pub fn new(config: HoldConfig) -> Self {
        let agent_id = config.resolve_agent_id();
        Self {
            config,
            connection: Arc::new(RwLock::new(None)),
            channel: Arc::new(RwLock::new(None)),
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            agent_id,
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    pub async fn state(&self) -> ConnectionState {
        self.state.read().await.clone()
    }

    pub async fn is_connected(&self) -> bool {
        matches!(*self.state.read().await, ConnectionState::Connected)
    }

    pub async fn connect(&self) -> Result<()> {
        if !self.config.is_configured() {
            return Err(anyhow::anyhow!("HOLD integration not configured"));
        }

        let uri = self
            .config
            .get_connection_uri()
            .ok_or_else(|| anyhow::anyhow!("No HOLD endpoint configured"))?;

        *self.state.write().await = ConnectionState::Connecting;

        let mut retry_count = 0;
        let max_attempts = self.config.retry.max_attempts;
        let mut backoff = self.config.retry.backoff_secs;

        loop {
            info!(
                "Connecting to HOLD (attempt {}/{})",
                retry_count + 1,
                max_attempts
            );

            match self.try_connect(&uri).await {
                Ok(()) => {
                    *self.state.write().await = ConnectionState::Connected;
                    info!("Connected to HOLD successfully");
                    return Ok(());
                }
                Err(e) => {
                    retry_count += 1;
                    let reason = format!("{}", e);

                    if retry_count >= max_attempts {
                        *self.state.write().await = ConnectionState::Failed {
                            reason: reason.clone(),
                            retry_count,
                        };
                        warn!("Failed to connect to HOLD after {} attempts", max_attempts);
                        return Err(e);
                    }

                    *self.state.write().await = ConnectionState::Failed {
                        reason,
                        retry_count,
                    };

                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff as f64 * self.config.retry.backoff_multiplier) as u64;
                    backoff = backoff.min(self.config.retry.max_backoff_secs);
                }
            }
        }
    }

    async fn try_connect(&self, uri: &str) -> Result<()> {
        let connection = Connection::connect(uri, ConnectionProperties::default())
            .await
            .context("Failed to connect to HOLD AMQP broker")?;

        let channel = connection
            .create_channel()
            .await
            .context("Failed to create AMQP channel")?;

        channel
            .exchange_declare(
                "warden.hold",
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .context("Failed to declare HOLD exchange")?;

        let queue_name = format!("warden.commands.hold.{}", self.agent_id);
        channel
            .queue_declare(
                &queue_name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .context("Failed to declare command queue")?;

        channel
            .queue_bind(
                &queue_name,
                "warden.hold",
                &format!("warden.commands.hold.{}", self.agent_id),
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .context("Failed to bind command queue (direct agent routing key)")?;

        channel
            .queue_bind(
                &queue_name,
                "warden.hold",
                "warden.commands.hold.broadcast",
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .context("Failed to bind command queue (broadcast routing key)")?;

        *self.connection.write().await = Some(connection);
        *self.channel.write().await = Some(channel);
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        if let Some(connection) = self.connection.write().await.take() {
            connection
                .close(0, "Shutting down")
                .await
                .context("Failed to close HOLD connection")?;
        }
        *self.channel.write().await = None;
        *self.state.write().await = ConnectionState::Disconnected;
        info!("Disconnected from HOLD");
        Ok(())
    }

    pub async fn publish(&self, routing_key: &str, payload: &[u8]) -> Result<Option<Confirmation>> {
        let channel_guard = self.channel.read().await;
        let channel = match channel_guard.as_ref() {
            Some(ch) => ch,
            None => {
                debug!("HOLD not connected, skipping publish to {}", routing_key);
                return Ok(None);
            }
        };

        match channel
            .basic_publish(
                "warden.hold",
                routing_key,
                BasicPublishOptions::default(),
                payload,
                BasicProperties::default()
                    .with_content_type("application/json".into())
                    .with_delivery_mode(2),
            )
            .await
        {
            Ok(confirm) => match confirm.await {
                Ok(confirmation) => {
                    debug!("Published to HOLD: {}", routing_key);
                    Ok(Some(confirmation))
                }
                Err(e) => {
                    warn!("HOLD publish confirmation failed: {}", e);
                    Ok(None)
                }
            },
            Err(e) => {
                warn!("Failed to publish to HOLD: {}", e);
                let retry_count = match &*self.state.read().await {
                    ConnectionState::Failed { retry_count, .. } => *retry_count,
                    _ => 0,
                };
                *self.state.write().await = ConnectionState::Failed {
                    reason: format!("{}", e),
                    retry_count,
                };
                Ok(None)
            }
        }
    }

    pub async fn consume_commands(&self) -> Result<Option<lapin::Consumer>> {
        let channel_guard = self.channel.read().await;
        let channel = match channel_guard.as_ref() {
            Some(ch) => ch,
            None => return Ok(None),
        };

        let queue_name = format!("warden.commands.hold.{}", self.agent_id);
        let consumer = channel
            .basic_consume(
                &queue_name,
                &format!("warden-consumer-{}", self.agent_id),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .context("Failed to create HOLD command consumer")?;

        Ok(Some(consumer))
    }

    pub async fn ack(&self, delivery_tag: u64) -> Result<()> {
        let channel_guard = self.channel.read().await;
        if let Some(channel) = channel_guard.as_ref() {
            channel
                .basic_ack(delivery_tag, BasicAckOptions::default())
                .await?;
        }
        Ok(())
    }
}
