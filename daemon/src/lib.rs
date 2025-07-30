pub mod amqp;
pub mod cli;
pub mod handlers;

use amqp::AmqpClient;
use anyhow::{anyhow, Context, Result};
use common::config::WardenConfig;
use futures::StreamExt;
use lapin::message::Delivery;
use lapin::options::BasicAckOptions;
use log::{debug, error, info, warn};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task;

/// Main daemon struct that handles AMQP communication
#[derive()]
pub struct Daemon {
    config: Arc<Mutex<WardenConfig>>,
    amqp_client: Option<Arc<amqp::AmqpClient>>,
}

impl Daemon {
    /// Create a new daemon instance with the given configuration
    pub fn new(config: WardenConfig) -> Self {
        Daemon {
            config: Arc::new(Mutex::new(config)),
            amqp_client: None,
        }
    }

    /// Initialize the AMQP client with the current configuration
    pub async fn init_amqp(&mut self) -> Result<()> {
        // Extract config fields up front to avoid holding the MutexGuard across await
        let (c2_server, c2_auth_id, c2_auth_secret) = {
            let config_guard = self.config.lock().unwrap();
            (
                config_guard.c2_server.clone(),
                config_guard.c2_auth.id.clone(),
                config_guard.c2_auth.secret.clone(),
            )
        };
        // MutexGuard is dropped here before any await points

        // Create AMQP config from configuration
        // Use C2 server as default broker
        let _default_broker = format!("amqp://{c2_server}");

        // Create AMQP config based on available configuration
        let amqp_config = {
            // Extract host from C2 server URL
            let amqp_host = c2_server.clone();
            let amqp_host = amqp_host.replace("http://", "").replace("https://", "");

            // Split host and port
            let parts: Vec<&str> = amqp_host.split(':').collect();
            let host = parts[0].to_string();
            let port = if parts.len() > 1 {
                parts[1].parse::<u16>().unwrap_or(5672)
            } else {
                5672 // Default AMQP port
            };

            let username = if !c2_auth_id.is_empty() {
                Some(c2_auth_id.clone())
            } else {
                None
            };
            let password = if !c2_auth_secret.is_empty() {
                Some(c2_auth_secret.clone())
            } else {
                None
            };

            amqp::AmqpConfig {
                host,
                port,
                username,
                password,
                client_id: format!("warden-{c2_auth_id}"),
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
        };

        // Create AMQP client
        let client = amqp::AmqpClient::new(&amqp_config).await?;

        // Log connection details
        info!(
            "AMQP client initialized - connected to {}:{}",
            amqp_config.host, amqp_config.port
        );
        info!("Using exchange: {}", amqp_config.exchange);
        info!("Routing keys: {:?}", amqp_config.routing_keys);

        self.amqp_client = Some(Arc::new(client));

        Ok(())
    }

    /// Start the daemon and begin processing AMQP messages
    pub async fn start(&mut self) -> Result<()> {
        // Initialize AMQP client if not already done
        if self.amqp_client.is_none() {
            self.init_amqp().await?;
        }

        let client = self
            .amqp_client
            .as_ref()
            .ok_or_else(|| anyhow!("AMQP client not initialized"))?
            .clone();
        let config = Arc::clone(&self.config);

        // Get configuration for exchange, queues, and routing keys
        let (exchange, queue_bindings) = self.get_queue_configuration();

        // Set up AMQP infrastructure
        self.setup_amqp_infrastructure(&client, &exchange, &queue_bindings)
            .await?;

        // Create message processing channel
        let (tx, rx) = mpsc::channel::<(String, String, Delivery)>(100);

        // Spawn consumer tasks for each queue
        let consumer_tasks = self
            .spawn_consumer_tasks(&client, &queue_bindings, tx.clone())
            .await?;

        // Spawn message processing task
        let process_task = self
            .spawn_message_processor(rx, client.clone(), config)
            .await?;

        // Monitor tasks and handle unexpected termination
        self.monitor_tasks(consumer_tasks, process_task).await;

        Ok(())
    }

    /// Get queue configuration from config or use defaults
    fn get_queue_configuration(&self) -> (String, Vec<(String, String)>) {
        // Default values
        let default_exchange = "warden".to_string();
        let default_bindings = vec![
            (
                "warden.commands".to_string(),
                "warden.commands.#".to_string(),
            ),
            ("warden.config".to_string(), "warden.config".to_string()),
            ("warden.events".to_string(), "warden.events.#".to_string()),
        ];

        // TODO: In the future, we could extract these from self.config if custom values are needed

        (default_exchange, default_bindings)
    }

    /// Set up AMQP infrastructure (exchange, queues, bindings)
    async fn setup_amqp_infrastructure(
        &self,
        client: &Arc<AmqpClient>,
        exchange: &str,
        queue_bindings: &[(String, String)],
    ) -> Result<()> {
        // Declare exchange
        client
            .declare_exchange(exchange)
            .await
            .context("Failed to declare exchange")?;

        // Declare queues and bind to exchange with routing keys
        for (queue, routing_key) in queue_bindings {
            client
                .declare_queue(queue)
                .await
                .context(format!("Failed to declare queue {queue}"))?;

            client
                .bind_queue(queue, exchange, routing_key)
                .await
                .context({
                    format!(
                        "Failed to bind queue {queue} to exchange {exchange} with routing key {routing_key}"
                    )
                })?;

            info!("Bound queue {queue} to exchange {exchange} with routing key {routing_key}");
        }

        Ok(())
    }

    /// Spawn consumer tasks for each queue
    async fn spawn_consumer_tasks(
        &self,
        client: &Arc<AmqpClient>,
        queue_bindings: &[(String, String)],
        tx: mpsc::Sender<(String, String, Delivery)>,
    ) -> Result<Vec<task::JoinHandle<()>>> {
        let mut consumer_tasks = Vec::new();

        for (queue, _) in queue_bindings {
            let queue_name = queue.clone();
            let client_clone = client.clone();
            let tx_clone = tx.clone();

            let task = task::spawn(async move {
                loop {
                    match client_clone.consume(&queue_name).await {
                        Ok(mut consumer) => {
                            info!("Started consuming from queue: {queue_name}");

                            while let Some(delivery_result) = consumer.next().await {
                                match delivery_result {
                                    Ok(delivery) => {
                                        let routing_key = delivery.routing_key.to_string();
                                        let payload =
                                            String::from_utf8_lossy(&delivery.data).to_string();

                                        debug!(
                                            "Received message on routing key {routing_key}: {payload}"
                                        );

                                        // Get delivery tag before moving the delivery
                                        let delivery_tag = delivery.delivery_tag;

                                        // Send to processing channel
                                        if let Err(e) = tx_clone
                                            .send((queue_name.clone(), routing_key, delivery))
                                            .await
                                        {
                                            error!(
                                                "Failed to send message to processing channel: {e}"
                                            );
                                        }

                                        // Acknowledge the message
                                        let channel = client_clone.channel();
                                        let channel_guard = channel.lock().await;
                                        if let Err(e) = channel_guard
                                            .basic_ack(delivery_tag, BasicAckOptions::default())
                                            .await
                                        {
                                            error!("Failed to acknowledge message: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        error!(
                                            "Error receiving message from queue {queue_name}: {e}"
                                        );
                                    }
                                }
                            }

                            // If we get here, the consumer has ended - we'll try to reconnect
                            warn!("Consumer for queue {queue_name} ended unexpectedly, reconnecting in 5 seconds...");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                        Err(e) => {
                            error!("Failed to consume from queue {queue_name}: {e}");
                            // Wait before retrying
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            });

            consumer_tasks.push(task);
        }

        Ok(consumer_tasks)
    }

    /// Spawn message processor task
    async fn spawn_message_processor(
        &self,
        mut rx: mpsc::Receiver<(String, String, Delivery)>,
        client: Arc<AmqpClient>,
        config: Arc<Mutex<WardenConfig>>,
    ) -> Result<task::JoinHandle<()>> {
        let process_task = task::spawn(async move {
            while let Some((queue, routing_key, delivery)) = rx.recv().await {
                let payload = String::from_utf8_lossy(&delivery.data).to_string();

                let result = match true {
                    // Command handling
                    _ if queue.contains("commands") || routing_key.contains("commands") => {
                        handlers::command::handle_command(&routing_key, &payload, &client, &config)
                            .await
                    }
                    // Config update handling
                    _ if queue.contains("config") || routing_key.contains("config") => {
                        handlers::config::handle_config_update(&payload, &config).await
                    }
                    // Event handling
                    _ if queue.contains("events") || routing_key.contains("events") => {
                        handlers::event::handle_event(&routing_key, &payload, &client, &config)
                            .await
                    }
                    // Unknown message type
                    _ => {
                        warn!("Received message on unknown routing key: {routing_key}");
                        Ok(())
                    }
                };

                if let Err(e) = result {
                    error!("Error processing message on routing key {routing_key}: {e}");
                }
            }
        });

        Ok(process_task)
    }

    /// Monitor tasks and handle unexpected termination
    async fn monitor_tasks(
        &self,
        consumer_tasks: Vec<task::JoinHandle<()>>,
        process_task: task::JoinHandle<()>,
    ) {
        // Create a future that completes when any consumer task completes
        let consumer_future = async {
            for (i, task) in futures::future::join_all(consumer_tasks)
                .await
                .into_iter()
                .enumerate()
            {
                if let Err(e) = task {
                    error!("Consumer task {i} failed: {e}");
                }
            }
        };

        // Create a future that completes when the process task completes
        let process_future = async {
            if let Err(e) = process_task.await {
                error!("Message processing task failed: {e}");
            }
        };

        // Wait for either future to complete
        tokio::select! {
            _ = consumer_future => {
                error!("Consumer tasks ended unexpectedly");
            },
            _ = process_future => {
                error!("Message processing task ended unexpectedly");
            }
        }
    }

    /// Stop the daemon and clean up resources
    pub async fn stop(&self) -> Result<()> {
        if let Some(client) = &self.amqp_client {
            // Determine status exchange and routing key from config
            let (exchange, status_routing_key) = {
                let config_guard = self.config.lock().unwrap();
                // Default values
                let default_exchange = "warden".to_string();
                let default_status_key = "warden.status".to_string();

                if let Some(mqtt_config) = &config_guard.mqtt {
                    let exchange = mqtt_config
                        .exchange
                        .clone()
                        .unwrap_or_else(|| "warden".to_string());

                    // Check if a status topic is defined in the topics list
                    let status_key = mqtt_config
                        .topics
                        .as_ref()
                        .and_then(|topics| {
                            topics
                                .iter()
                                .find(|t| t.contains("status"))
                                .cloned()
                                .or(Some("warden.status".to_string()))
                        })
                        .unwrap_or_else(|| "warden.status".to_string());

                    (exchange, status_key.to_string())
                } else {
                    (default_exchange, default_status_key)
                }
            };

            // Publish a last will message
            match client
                .publish(
                    &exchange,
                    &status_routing_key,
                    amqp::MessageType::Response,
                    "offline",
                )
                .await
            {
                Ok(_) => info!("Published offline status"),
                Err(e) => error!("Failed to publish offline status: {e}"),
            }

            // Close connection gracefully
            if let Err(e) = client.close().await {
                error!("Error closing AMQP connection: {e}");
            }
        }

        info!("Daemon stopped");
        Ok(())
    }

    /// Get a reference to the current configuration
    pub fn config(&self) -> Arc<Mutex<WardenConfig>> {
        Arc::clone(&self.config)
    }
}
