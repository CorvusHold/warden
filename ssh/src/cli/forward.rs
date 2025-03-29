use anyhow::Result;
use clap::Parser;
use crate::ssh::SSHTunnel;
use log::{info, warn};
use crate::SshError;
use std::{thread, net::TcpListener, sync::Arc};
use ctrlc;

/// Forward a remote port to a local port over SSH.
#[derive(Parser, Debug)]
#[clap(name = "forward", about = "Forward a remote port to a local port over SSH")]
pub struct ForwardCommand {
    /// The SSH username.
    #[clap(short = 'U', long, default_value = "root")]
    ssh_user: String,

    /// The SSH server address.
    #[clap(short = 'H', long)]
    ssh_host: String,

    /// The SSH server port.
    #[clap(short = 'P', long, default_value = "22")]
    ssh_port: u16,

    /// The local port to listen on.
    #[clap(long)]
    local_port: Option<u16>,

    /// The remote host to forward to.
    #[clap(long)]
    remote_host: String,

    /// The remote port to forward.
    #[clap(long)]
    remote_port: u16,

    /// The remote password for SSH authentication.
    #[clap(long)]
    remote_password: Option<String>,

    /// The path to the private key for SSH authentication.
    #[clap(long)]
    remote_key_path: Option<String>,
}

/// Find an available local port
pub fn find_available_port() -> Option<u16> {
    (10000..65535).find(|port| TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok())
}

pub async fn forward(cmd: ForwardCommand) -> Result<()> {
    let ForwardCommand { ssh_user, ssh_host, ssh_port, local_port, remote_host, remote_port, remote_password, remote_key_path } = cmd;

    // Get local port (either specified or find available)
    let local_port = local_port.unwrap_or_else(|| find_available_port().expect("No available ports found"));

    println!("Forwarding remote port {} on {} to local port {}", remote_port, remote_host, local_port);

    let mut tunnel = SSHTunnel::new(ssh_host.clone(), ssh_user.clone(), Some(ssh_port));

    info!("Attempting SSH tunnel to {}@{}:{}", ssh_user, ssh_host, ssh_port);

    // Set authentication
    if let Some(password) = &remote_password {
        info!("Using SSH password authentication");
        tunnel = tunnel.with_password(password.clone());
    } else if let Some(key_path) = &remote_key_path {
        info!("Using SSH key authentication from {}", key_path);
        tunnel = tunnel.with_private_key_path(key_path.clone());
    } else {
        return Err(SshError::ConfigurationError(
            "Either SSH password or key path must be specified".to_string()
        ).into());
    }

    // Get a reference to the tunnel's running flag
    let tunnel_ref = Arc::new(tunnel);
    let tunnel_weak = Arc::downgrade(&tunnel_ref);
    
    // Set up Ctrl+C handler that will stop the tunnel
    ctrlc::set_handler(move || {
        info!("Received Ctrl+C, shutting down tunnel...");
        if let Some(tunnel) = tunnel_weak.upgrade() {
            if let Err(e) = tunnel.stop() {
                warn!("Error stopping SSH tunnel: {}", e);
            } else {
                info!("SSH tunnel closed successfully");
            }
        }
    }).expect("Error setting Ctrl+C handler");

    // Forward the port
    info!("Forwarding port {} to {}:{}", local_port, remote_host, remote_port);
    match tunnel_ref.forward_port(local_port, remote_port, remote_host.clone()).await {
        Ok(_) => {
            println!("SSH tunnel established successfully");
            println!("Connect to localhost:{} to access {}:{}", local_port, remote_host, remote_port);
            
            // Keep the tunnel running until it's stopped (e.g., by Ctrl+C)
            while tunnel_ref.is_running() {
                thread::sleep(std::time::Duration::from_secs(1));
            }
            
            info!("SSH tunnel has been closed");
            Ok(())
        },
        Err(e) => {
            warn!("SSH tunnel error: {}", e);
            Err(SshError::TunnelError(e.to_string()).into())
        }
    }
}
