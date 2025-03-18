use anyhow::{Result, Context, anyhow};
use lapin::{options::*, types::FieldTable, BasicProperties, Connection, ConnectionProperties, Channel, ExchangeKind, publisher_confirm::Confirmation};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use common::config::WardenConfig;
use log::{info, debug, error};
use std::time::{SystemTime, UNIX_EPOCH};

/// Re-export MessageType from amqp module to maintain compatibility
pub use crate::amqp::MessageType;

/// Re-export Message from amqp module to maintain compatibility
pub use crate::amqp::Message;

/// MQTT client wrapper using lapin
pub struct MqttClient {
    connection: Connection,
    channel: Arc<Mutex<Channel>>,
}

impl MqttClient {
    /// Create a new MQTT client using lapin
    pub async fn new(config: &MqttConfig) -> Result<Self> {
        let uri = format!(
            "amqp://{}:{}@{}:{}/",
            config.username.as_deref().unwrap_or("guest"),
            config.password.as_deref().unwrap_or("guest"),
            config.host,
            config.port
        );
        
        info!("Connecting to MQTT broker at {}", uri);
        
        let connection = Connection::connect(
            &uri,
            ConnectionProperties::default(),
        ).await.context("Failed to connect to MQTT broker")?;
        
        let channel = connection.create_channel().await
            .context("Failed to create MQTT channel")?;
        
        Ok(Self {
            connection,
            channel: Arc::new(Mutex::new(channel)),
        })
    }
    
    /// Declare an exchange
    pub async fn declare_exchange(&self, name: &str) -> Result<()> {
        let channel = self.channel.lock().await;
        channel.exchange_declare(
            name,
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        ).await.context("Failed to declare exchange")?;
        
        debug!("Declared exchange: {}", name);
        Ok(())
    }
    
    /// Publish a message to a topic
    pub async fn publish<T: Serialize>(
        &self, 
        exchange: &str, 
        topic: &str, 
        message_type: MessageType,
        payload: T
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
        
        let json = serde_json::to_string(&message)
            .context("Failed to serialize message")?;
        
        let channel = self.channel.lock().await;
        let confirm = channel.basic_publish(
            exchange,
            topic,
            BasicPublishOptions::default(),
            json.as_bytes(),
            BasicProperties::default(),
        ).await.context("Failed to publish message")?
        .await.context("Failed to get publish confirmation")?;
        
        if confirm.is_ack() {
            debug!("Published message to {}/{}: {}", exchange, topic, json);
            Ok(confirm)
        } else {
            Err(anyhow!("Message was not acknowledged by the broker"))
        }
    }
    
    /// Subscribe to a topic
    pub async fn subscribe(&self, queue: &str) -> Result<lapin::Consumer> {
        let channel = self.channel.lock().await;
        let consumer = channel.basic_consume(
            queue,
            &format!("consumer-{}", uuid::Uuid::new_v4()),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        ).await.context("Failed to subscribe to topic")?;
        
        debug!("Subscribed to topic: {}", queue);
        Ok(consumer)
    }
    
    /// Get the channel
    pub fn channel(&self) -> Arc<Mutex<Channel>> {
        self.channel.clone()
    }
    
    /// Close the connection
    pub async fn close(&self) -> Result<()> {
        self.connection.close(0, "").await
            .context("Failed to close MQTT connection")
    }
}

/// Helper struct for MQTT operations
pub struct MqttHelper;

impl MqttHelper {
    /// Parse a message from JSON
    pub fn parse_message<T: for<'de> Deserialize<'de>>(payload: &str) -> Result<Message<T>> {
        serde_json::from_str(payload)
            .context("Failed to parse message payload")
    }
    
    /// Extract the subtopic from a topic
    pub fn get_subtopic(topic: &str, prefix: &str) -> Option<String> {
        if topic.starts_with(prefix) {
            Some(topic[prefix.len()..].to_string())
        } else {
            None
        }
    }
}

/// Struct to hold MQTT connection details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client_id: String,
}

impl MqttConfig {
    /// Create MQTT config from WardenConfig
    pub fn from_warden_config(config: &WardenConfig) -> Self {
        // Extract MQTT broker details from the C2 server URL
        let mqtt_host = config.c2_server.clone();
        let mqtt_host = mqtt_host.replace("http://", "").replace("https://", "");
        
        // Split host and port
        let parts: Vec<&str> = mqtt_host.split(':').collect();
        let host = parts[0].to_string();
        let port = if parts.len() > 1 {
            parts[1].parse::<u16>().unwrap_or(1883)
        } else {
            1883 // Default MQTT port
        };
        
        // Create MQTT config
        MqttConfig {
            host,
            port,
            username: if !config.c2_auth.id.is_empty() { Some(config.c2_auth.id.clone()) } else { None },
            password: if !config.c2_auth.secret.is_empty() { Some(config.c2_auth.secret.clone()) } else { None },
            client_id: format!("warden-{}", config.c2_auth.id),
        }
    }
}
