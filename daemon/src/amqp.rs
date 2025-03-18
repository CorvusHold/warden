use anyhow::{anyhow, Context, Result};
use common::config::WardenConfig;
use lapin::{
    options::*, publisher_confirm::Confirmation, types::FieldTable, BasicProperties, Channel,
    Connection, ConnectionProperties, ExchangeKind,
};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// AMQP message types that the daemon can handle
#[derive(Debug, Serialize, Deserialize)]
pub enum MessageType {
    Command,
    Event,
    ConfigUpdate,
    Response,
    Notification,
    Acknowledgment,
}

/// Standard message format for AMQP communication
#[derive(Debug, Serialize, Deserialize)]
pub struct Message<T> {
    pub message_type: MessageType,
    pub timestamp: u64,
    pub payload: T,
}

/// AMQP client wrapper
pub struct AmqpClient {
    connection: Connection,
    channel: Arc<Mutex<Channel>>,
}

impl AmqpClient {
    /// Create a new AMQP client
    pub async fn new(config: &AmqpConfig) -> Result<Self> {
        let uri = format!(
            "amqp://{}:{}@{}:{}/{}",
            config.username.as_deref().unwrap_or("guest"),
            config.password.as_deref().unwrap_or("guest"),
            config.host,
            config.port,
            config.vhost.as_deref().unwrap_or("")
        );

        info!("Connecting to AMQP broker at {}", uri);

        let connection = Connection::connect(&uri, ConnectionProperties::default())
            .await
            .context("Failed to connect to AMQP broker")?;

        let channel = connection
            .create_channel()
            .await
            .context("Failed to create AMQP channel")?;

        Ok(Self {
            connection,
            channel: Arc::new(Mutex::new(channel)),
        })
    }

    /// Declare an exchange
    pub async fn declare_exchange(&self, name: &str) -> Result<()> {
        let channel = self.channel.lock().await;
        channel
            .exchange_declare(
                name,
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .context("Failed to declare exchange")?;

        debug!("Declared exchange: {}", name);
        Ok(())
    }

    /// Declare a queue
    pub async fn declare_queue(&self, name: &str) -> Result<()> {
        let channel = self.channel.lock().await;
        channel
            .queue_declare(
                name,
                QueueDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .context("Failed to declare queue")?;

        debug!("Declared queue: {}", name);
        Ok(())
    }

    /// Bind a queue to an exchange with a routing key
    pub async fn bind_queue(&self, queue: &str, exchange: &str, routing_key: &str) -> Result<()> {
        let channel = self.channel.lock().await;
        channel
            .queue_bind(
                queue,
                exchange,
                routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
            .context("Failed to bind queue to exchange")?;

        debug!(
            "Bound queue {} to exchange {} with routing key {}",
            queue, exchange, routing_key
        );
        Ok(())
    }

    /// Publish a message to an exchange with a routing key
    pub async fn publish<T: Serialize>(
        &self,
        exchange: &str,
        routing_key: &str,
        message_type: MessageType,
        payload: T,
    ) -> Result<Confirmation> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let message = Message {
            message_type,
            timestamp,
            payload,
        };

        let json = serde_json::to_string(&message).context("Failed to serialize message")?;

        let channel = self.channel.lock().await;
        let confirm = channel
            .basic_publish(
                exchange,
                routing_key,
                BasicPublishOptions::default(),
                json.as_bytes(),
                BasicProperties::default(),
            )
            .await
            .context("Failed to publish message")?
            .await
            .context("Failed to get publish confirmation")?;

        if confirm.is_ack() {
            debug!(
                "Published message to {}/{}: {}",
                exchange, routing_key, json
            );
            Ok(confirm)
        } else {
            Err(anyhow!("Message was not acknowledged by the broker"))
        }
    }

    /// Consume messages from a queue
    pub async fn consume(&self, queue: &str) -> Result<lapin::Consumer> {
        let channel = self.channel.lock().await;
        let consumer = channel
            .basic_consume(
                queue,
                &format!("consumer-{}", uuid::Uuid::new_v4()),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .context("Failed to consume from queue")?;

        debug!("Started consuming from queue: {}", queue);
        Ok(consumer)
    }

    /// Get the channel
    pub fn channel(&self) -> Arc<Mutex<Channel>> {
        self.channel.clone()
    }

    /// Close the connection
    pub async fn close(&self) -> Result<()> {
        self.connection
            .close(0, "")
            .await
            .context("Failed to close AMQP connection")
    }
}

/// Helper struct for AMQP operations
pub struct AmqpHelper;

impl AmqpHelper {
    /// Parse a message from JSON
    pub fn parse_message<T: for<'de> Deserialize<'de>>(payload: &str) -> Result<Message<T>> {
        serde_json::from_str(payload).context("Failed to parse message payload")
    }

    /// Extract the routing key from a topic
    pub fn get_routing_key(topic: &str, prefix: &str) -> Option<String> {
        if topic.starts_with(prefix) {
            Some(topic[prefix.len()..].to_string())
        } else {
            None
        }
    }
}

/// Struct to hold AMQP connection details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmqpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client_id: String,
    pub vhost: Option<String>,
    pub exchange: String,
    pub queues: Vec<String>,
    pub routing_keys: Vec<String>,
}

impl AmqpConfig {
    /// Create AMQP config from WardenConfig
    pub fn from_warden_config(config: &WardenConfig) -> Self {
        // Extract AMQP broker details from the C2 server URL
        let amqp_host = config.c2_server.clone();
        let amqp_host = amqp_host.replace("http://", "").replace("https://", "");

        // Split host and port
        let parts: Vec<&str> = amqp_host.split(':').collect();
        let host = parts[0].to_string();
        let port = if parts.len() > 1 {
            parts[1].parse::<u16>().unwrap_or(5672)
        } else {
            5672 // Default AMQP port
        };

        // Create AMQP config
        AmqpConfig {
            host,
            port,
            username: if !config.c2_auth.id.is_empty() {
                Some(config.c2_auth.id.clone())
            } else {
                None
            },
            password: if !config.c2_auth.secret.is_empty() {
                Some(config.c2_auth.secret.clone())
            } else {
                None
            },
            client_id: format!("warden-{}", config.c2_auth.id),
            vhost: Some("/".to_string()),
            exchange: "warden".to_string(),
            queues: vec![
                "warden.commands".to_string(),
                "warden.config".to_string(),
                "warden.events".to_string(),
            ],
            routing_keys: vec![
                "warden.commands.#".to_string(),
                "warden.config".to_string(),
                "warden.events.#".to_string(),
            ],
        }
    }

    /// Create AMQP config from MQTT section in WardenConfig
    pub fn from_mqtt_config(config: &WardenConfig) -> Option<Self> {
        let mqtt = config.mqtt.as_ref()?;

        Some(AmqpConfig {
            host: mqtt.broker.clone(),
            port: mqtt.port.unwrap_or(5672),
            username: mqtt.username.clone(),
            password: mqtt.password.clone(),
            client_id: mqtt
                .client_id
                .clone()
                .unwrap_or_else(|| format!("warden-{}", uuid::Uuid::new_v4())),
            vhost: Some("/".to_string()),
            exchange: "warden".to_string(),
            queues: vec![
                "warden.commands".to_string(),
                "warden.config".to_string(),
                "warden.events".to_string(),
            ],
            routing_keys: mqtt.topics.clone().unwrap_or_else(|| {
                vec![
                    "warden.commands.#".to_string(),
                    "warden.config".to_string(),
                    "warden.events.#".to_string(),
                ]
            }),
        })
    }
}
