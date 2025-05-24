use crate::common::PostgresConfig;
use log::warn;
use log::{error, info};
use ssh::cli::forward::find_available_port;
use ssh::SSHTunnel;
use ssh::SshError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::OnceCell;

static TUNNEL_KEEPER: OnceCell<Arc<Mutex<TunnelKeeper>>> = OnceCell::const_new();

pub struct TunnelKeeper {
    pub tunnel: Option<SSHTunnel>,
    pub tunnel_thread: Option<thread::JoinHandle<()>>,
    pub tunnel_tx: Option<tokio::sync::mpsc::Sender<TunnelCommand>>,
    pub original_host: String,
    pub original_port: u16,
    pub is_active: AtomicBool,
}

#[allow(dead_code)]
pub enum TunnelCommand {
    Stop,
    Verify,
}

impl TunnelKeeper {
    pub async fn instance() -> Arc<Mutex<TunnelKeeper>> {
        TUNNEL_KEEPER
            .get_or_init(|| async {
                Arc::new(Mutex::new(TunnelKeeper {
                    tunnel: None,
                    tunnel_thread: None,
                    tunnel_tx: None,
                    original_host: String::new(),
                    original_port: 0,
                    is_active: AtomicBool::new(false),
                }))
            })
            .await
            .clone()
    }

    pub async fn setup(&mut self, config: &PostgresConfig) -> Result<(), SshError> {
        if self.is_active.load(Ordering::SeqCst) {
            info!("SSH tunnel is already active");
            return Ok(());
        }

        if let Some(_ssh_host) = &config.ssh_host {
            let local_port = config
                .ssh_local_port
                .unwrap_or_else(|| find_available_port().expect("No available ports found"));

            // Clone necessary config values
            let host = config.host.clone();
            let _port = config.port;
            let ssh_host = config.ssh_host.clone();
            let ssh_user = config.ssh_user.clone();
            let ssh_port = config.ssh_port;
            let ssh_password = config.ssh_password.clone();
            let ssh_key_path = config.ssh_key_path.clone();

            let mut tunnel = SSHTunnel::new(
                ssh_host.expect("SSH host must be specified"),
                ssh_user.expect("SSH user must be specified"),
                ssh_port,
            );

            // Set authentication
            if let Some(password) = &ssh_password {
                tunnel = tunnel.with_password(password.clone());
            } else if let Some(key_path) = &ssh_key_path {
                tunnel = tunnel.with_private_key_path(key_path.clone());
            } else {
                return Err(SshError::ConfigurationError(
                    "Either SSH password or key path must be specified".to_string(),
                ));
            }

            // Store original connection details
            self.original_host = host.clone();
            self.original_port = config.ssh_remote_port.ok_or(SshError::ConfigurationError(
                "SSH remote port must be specified".to_string(),
            ))?;

            let host = config.host.clone();
            let port = config.ssh_remote_port.ok_or(SshError::ConfigurationError(
                "SSH remote port must be specified".to_string(),
            ))?;
            let ssh_host = config.ssh_host.clone().ok_or(SshError::ConfigurationError(
                "SSH host must be specified".to_string(),
            ))?;
            let ssh_user = config.ssh_user.clone().ok_or(SshError::ConfigurationError(
                "SSH user must be specified".to_string(),
            ))?;
            let ssh_port = config.ssh_port;
            let ssh_password = config.ssh_password.clone();
            let ssh_key_path = config.ssh_key_path.clone();
            // Before creating the tunnel, clone the values for logging

            // Start tunnel in background thread
            let handle = thread::spawn(move || {
                let runtime = tokio::runtime::Runtime::new().unwrap();
                let _ = runtime.block_on(async move {
                    // Create new tunnel instance for the thread
                    let ssh_user_clone = ssh_user.clone();
                    let ssh_host_clone = ssh_host.clone();

                    // Use the cloned values in the info macro
                    info!(
                        "Setting up SSH tunnel from localhost:{} to {}:{} via {}@{}",
                        local_port, host, port, ssh_user_clone, ssh_host_clone
                    );

                    // Create the tunnel with the original values
                    let mut tunnel = SSHTunnel::new(ssh_host.clone(), ssh_user.clone(), ssh_port);
                    if let Some(password) = &ssh_password {
                        tunnel = tunnel.with_password(password.clone());
                    } else if let Some(key_path) = &ssh_key_path {
                        tunnel = tunnel.with_private_key_path(key_path.clone());
                    }

                    // In the tunnel setup, before starting the tunnel:
                    info!(
                        "Setting up SSH tunnel from localhost:{} to {}:{} via {}@{}",
                        local_port, host, port, ssh_user_clone, ssh_host_clone
                    );

                    // In the tunnel forward_port call, add more detailed logging:
                    info!(
                        "Forwarding local port {} to remote {}:{}",
                        local_port, host, port
                    );
                    if let Err(e) = tunnel.forward_port(local_port, port, host).await {
                        error!("SSH tunnel forwarding failed: {}", e);
                        return Err(SshError::TunnelError(format!(
                            "Failed to forward port: {}",
                            e
                        )));
                    };
                    Ok(())
                });
            });

            // Store tunnel components
            self.tunnel = Some(tunnel); // Store the original tunnel
            self.tunnel_thread = Some(handle);
            self.is_active.store(true, Ordering::SeqCst);

            // In the setup method, right after:
            self.is_active.store(true, Ordering::SeqCst);
            info!("SSH tunnel established successfully");

            // Add this verification code:
            match self.verify_tunnel().await {
                Ok(_) => info!("SSH tunnel verified successfully"),
                Err(e) => {
                    error!("Failed to verify SSH tunnel: {}", e);
                    return Err(e);
                }
            }

            // Then continue with the existing return:
            Ok(())
        } else {
            Ok(())
        }
    }

    pub async fn verify_tunnel(&self) -> Result<(), SshError> {
        if !self.is_active.load(Ordering::SeqCst) {
            return Err(SshError::TunnelError("Tunnel is not active".to_string()));
        }

        let mut attempts = 3;
        while attempts > 0 {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            info!("Verifying tunnel connection (attempt {})", 4 - attempts);

            // Use pg_isready to check server availability
            let status = std::process::Command::new("pg_isready")
                .arg("-h")
                .arg("localhost")
                .arg("-p")
                .arg(self.original_port.to_string())
                .status();

            match status {
                Ok(exit_status) if exit_status.success() => {
                    info!("Successfully verified PostgreSQL server availability");
                    return Ok(());
                }
                Ok(_) => {
                    warn!("PostgreSQL server not ready");
                    attempts -= 1;
                }
                Err(e) => {
                    warn!("Failed to run pg_isready: {}", e);
                    attempts -= 1;
                }
            }
        }
        Err(SshError::TunnelError(
            "Failed to verify PostgreSQL server availability".to_string(),
        ))
    }

    pub async fn close(&mut self) -> Result<(), SshError> {
        if !self.is_active.load(Ordering::SeqCst) {
            return Ok(());
        }

        if let Some(tunnel) = self.tunnel.take() {
            tunnel
                .stop()
                .map_err(|e| SshError::TunnelError(format!("Error closing tunnel: {}", e)))?;
            self.is_active.store(false, Ordering::SeqCst);
            info!("SSH tunnel closed successfully");
        }
        Ok(())
    }
}
