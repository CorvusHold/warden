//! Pre-flight checks for HA operations.
//!
//! This module provides utilities for checking node health, replication status,
//! and other preconditions before executing HA operations.

use common::config::{Node, NodeRole};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

use super::types::HaError;

/// Health status of a PostgreSQL node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeHealthStatus {
    /// Node is healthy and accepting connections.
    Healthy,
    /// Node is reachable but not accepting connections.
    Degraded,
    /// Node is not reachable.
    Unreachable,
    /// Health status is unknown.
    Unknown,
}

impl std::fmt::Display for NodeHealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeHealthStatus::Healthy => write!(f, "healthy"),
            NodeHealthStatus::Degraded => write!(f, "degraded"),
            NodeHealthStatus::Unreachable => write!(f, "unreachable"),
            NodeHealthStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Result of a node health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealthCheck {
    /// Node ID.
    pub node_id: String,
    /// Node host.
    pub host: String,
    /// Node port.
    pub port: u16,
    /// Health status.
    pub status: NodeHealthStatus,
    /// Actual role detected (if reachable).
    pub detected_role: Option<NodeRole>,
    /// Error message if check failed.
    pub error: Option<String>,
    /// Response time in milliseconds.
    pub response_time_ms: Option<u64>,
}

impl NodeHealthCheck {
    /// Create a new health check result.
    pub fn new(node_id: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self {
            node_id: node_id.into(),
            host: host.into(),
            port,
            status: NodeHealthStatus::Unknown,
            detected_role: None,
            error: None,
            response_time_ms: None,
        }
    }

    /// Mark as healthy.
    pub fn healthy(mut self, response_time_ms: u64) -> Self {
        self.status = NodeHealthStatus::Healthy;
        self.response_time_ms = Some(response_time_ms);
        self
    }

    /// Mark as unreachable.
    pub fn unreachable(mut self, error: impl Into<String>) -> Self {
        self.status = NodeHealthStatus::Unreachable;
        self.error = Some(error.into());
        self
    }

    /// Set detected role.
    pub fn with_role(mut self, role: NodeRole) -> Self {
        self.detected_role = Some(role);
        self
    }
}

/// Replication status for a replica node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationStatus {
    /// Node ID.
    pub node_id: String,
    /// Whether the node is in recovery mode.
    pub is_in_recovery: bool,
    /// Replication lag in bytes (if available).
    pub lag_bytes: Option<u64>,
    /// Replication lag in seconds (if available).
    pub lag_seconds: Option<f64>,
    /// Last received LSN.
    pub received_lsn: Option<String>,
    /// Last replayed LSN.
    pub replayed_lsn: Option<String>,
    /// Whether replication is streaming.
    pub is_streaming: bool,
}

impl ReplicationStatus {
    /// Create a new replication status.
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            is_in_recovery: false,
            lag_bytes: None,
            lag_seconds: None,
            received_lsn: None,
            replayed_lsn: None,
            is_streaming: false,
        }
    }

    /// Check if replication lag is acceptable (< 1MB by default).
    pub fn is_lag_acceptable(&self, max_lag_bytes: u64) -> bool {
        match self.lag_bytes {
            Some(lag) => lag <= max_lag_bytes,
            None => true, // Assume acceptable if we can't measure
        }
    }
}

/// Check if a PostgreSQL node is reachable using pg_isready.
pub fn check_node_reachable(node: &Node, timeout_secs: u64) -> NodeHealthCheck {
    let mut check = NodeHealthCheck::new(&node.id, &node.host, node.port);

    info!(
        "[ha-check] Checking node {} at {}:{}",
        node.id, node.host, node.port
    );

    let start = std::time::Instant::now();

    // Use pg_isready to check connectivity
    let result = Command::new("pg_isready")
        .arg("-h")
        .arg(&node.host)
        .arg("-p")
        .arg(node.port.to_string())
        .arg("-t")
        .arg(timeout_secs.to_string())
        .output();

    let elapsed_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(output) => {
            if output.status.success() {
                debug!(
                    "[ha-check] Node {} is reachable ({}ms)",
                    node.id, elapsed_ms
                );
                check = check.healthy(elapsed_ms);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let error_msg = if !stderr.is_empty() {
                    stderr.to_string()
                } else if !stdout.is_empty() {
                    stdout.to_string()
                } else {
                    format!("pg_isready exited with code {:?}", output.status.code())
                };
                warn!("[ha-check] Node {} is unreachable: {}", node.id, error_msg);
                check = check.unreachable(error_msg);
            }
        }
        Err(e) => {
            let error_msg = format!("Failed to run pg_isready: {}", e);
            warn!("[ha-check] {}", error_msg);
            check = check.unreachable(error_msg);
        }
    }

    check
}

/// Check if a node is the primary (not in recovery mode).
pub async fn check_node_is_primary(
    host: &str,
    port: u16,
    user: &str,
    password: Option<&str>,
    database: &str,
) -> Result<bool, HaError> {
    let password_part = password.map(|p| format!(":{}", p)).unwrap_or_default();
    let conn_str = format!(
        "host={} port={} user={}{} dbname={}",
        host, port, user, password_part, database
    );

    debug!(
        "[ha-check] Checking if node is primary at {}:{}",
        host, port
    );

    // Use psql to check pg_is_in_recovery()
    let result = Command::new("psql")
        .arg(&conn_str)
        .arg("-t")
        .arg("-A")
        .arg("-c")
        .arg("SELECT NOT pg_is_in_recovery();")
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Ok(stdout == "t" || stdout == "true")
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(HaError::Postgres(format!(
                    "Failed to check primary status: {}",
                    stderr
                )))
            }
        }
        Err(e) => Err(HaError::Postgres(format!("Failed to run psql: {}", e))),
    }
}

/// Get replication status for a replica node.
pub async fn get_replication_status(
    node: &Node,
    user: &str,
    password: Option<&str>,
    database: &str,
) -> Result<ReplicationStatus, HaError> {
    let mut status = ReplicationStatus::new(&node.id);

    let password_part = password.map(|p| format!(":{}", p)).unwrap_or_default();
    let conn_str = format!(
        "host={} port={} user={}{} dbname={}",
        node.host, node.port, user, password_part, database
    );

    debug!(
        "[ha-check] Getting replication status for node {} at {}:{}",
        node.id, node.host, node.port
    );

    // Check if in recovery mode
    let recovery_result = Command::new("psql")
        .arg(&conn_str)
        .arg("-t")
        .arg("-A")
        .arg("-c")
        .arg("SELECT pg_is_in_recovery();")
        .output();

    match recovery_result {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                status.is_in_recovery = stdout == "t" || stdout == "true";
            }
        }
        Err(e) => {
            return Err(HaError::Postgres(format!(
                "Failed to check recovery status: {}",
                e
            )));
        }
    }

    // Get replication lag if in recovery mode
    if status.is_in_recovery {
        let lag_result = Command::new("psql")
            .arg(&conn_str)
            .arg("-t")
            .arg("-A")
            .arg("-c")
            .arg(
                "SELECT 
                    pg_wal_lsn_diff(pg_last_wal_receive_lsn(), pg_last_wal_replay_lsn()) as lag_bytes,
                    EXTRACT(EPOCH FROM (now() - pg_last_xact_replay_timestamp())) as lag_seconds,
                    pg_last_wal_receive_lsn()::text as received_lsn,
                    pg_last_wal_replay_lsn()::text as replayed_lsn;",
            )
            .output();

        if let Ok(output) = lag_result {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let parts: Vec<&str> = stdout.trim().split('|').collect();
                if parts.len() >= 4 {
                    status.lag_bytes = parts[0].trim().parse().ok();
                    status.lag_seconds = parts[1].trim().parse().ok();
                    status.received_lsn = Some(parts[2].trim().to_string());
                    status.replayed_lsn = Some(parts[3].trim().to_string());
                }
            }
        }

        // Check if streaming
        let streaming_result = Command::new("psql")
            .arg(&conn_str)
            .arg("-t")
            .arg("-A")
            .arg("-c")
            .arg("SELECT status FROM pg_stat_wal_receiver;")
            .output();

        if let Ok(output) = streaming_result {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                status.is_streaming = stdout == "streaming";
            }
        }
    }

    Ok(status)
}

/// Verify that a node matches the expected role.
pub fn verify_node_role(node: &Node, expected_role: NodeRole) -> Result<(), HaError> {
    if node.role != expected_role {
        return Err(HaError::InvalidNodeRole {
            node_id: node.id.clone(),
            actual_role: node.role.to_string(),
            expected_role: expected_role.to_string(),
        });
    }
    Ok(())
}

/// Wait for replication to catch up within a timeout.
pub async fn wait_for_replication_catchup(
    node: &Node,
    user: &str,
    password: Option<&str>,
    database: &str,
    max_lag_bytes: u64,
    timeout: Duration,
) -> Result<ReplicationStatus, HaError> {
    let start = std::time::Instant::now();
    let check_interval = Duration::from_secs(1);

    info!(
        "[ha-check] Waiting for replication catchup on node {} (max lag: {} bytes, timeout: {:?})",
        node.id, max_lag_bytes, timeout
    );

    loop {
        let status = get_replication_status(node, user, password, database).await?;

        if status.is_lag_acceptable(max_lag_bytes) {
            info!(
                "[ha-check] Replication caught up on node {} (lag: {:?} bytes)",
                node.id, status.lag_bytes
            );
            return Ok(status);
        }

        if start.elapsed() > timeout {
            return Err(HaError::ReplicationLagTooHigh {
                node_id: node.id.clone(),
                lag_bytes: status.lag_bytes.unwrap_or(0),
            });
        }

        debug!(
            "[ha-check] Replication lag on {}: {:?} bytes, waiting...",
            node.id, status.lag_bytes
        );
        tokio::time::sleep(check_interval).await;
    }
}

/// Promote a replica to primary using pg_ctl promote.
pub async fn promote_replica(data_dir: &str, timeout_secs: u64) -> Result<(), HaError> {
    info!("[ha-promote] Promoting replica at {}", data_dir);

    let result = Command::new("pg_ctl")
        .arg("promote")
        .arg("-D")
        .arg(data_dir)
        .arg("-W") // Don't wait
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                info!("[ha-promote] Promotion initiated successfully");

                // Wait for promotion to complete by checking for standby.signal removal
                let start = std::time::Instant::now();
                let standby_signal = std::path::Path::new(data_dir).join("standby.signal");

                while start.elapsed().as_secs() < timeout_secs {
                    if !standby_signal.exists() {
                        info!("[ha-promote] Promotion completed");
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }

                warn!("[ha-promote] Promotion may still be in progress after timeout");
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(HaError::StepFailed {
                    step: "promote_replica".to_string(),
                    reason: stderr.to_string(),
                })
            }
        }
        Err(e) => Err(HaError::StepFailed {
            step: "promote_replica".to_string(),
            reason: format!("Failed to run pg_ctl: {}", e),
        }),
    }
}

/// Create a standby.signal file to configure a node as a replica.
pub fn configure_as_replica(
    data_dir: &str,
    primary_host: &str,
    primary_port: u16,
    replication_user: &str,
) -> Result<(), HaError> {
    use std::fs;
    use std::io::Write;

    info!(
        "[ha-config] Configuring {} as replica of {}:{}",
        data_dir, primary_host, primary_port
    );

    // Create standby.signal
    let standby_signal = std::path::Path::new(data_dir).join("standby.signal");
    fs::File::create(&standby_signal)?;

    // Update postgresql.auto.conf with primary_conninfo
    let auto_conf = std::path::Path::new(data_dir).join("postgresql.auto.conf");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&auto_conf)?;

    writeln!(file, "\n# Added by Warden HA orchestration")?;
    writeln!(
        file,
        "primary_conninfo = 'host={} port={} user={}'",
        primary_host, primary_port, replication_user
    )?;

    info!("[ha-config] Replica configuration complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_health_check() {
        let check = NodeHealthCheck::new("node-1", "localhost", 5432)
            .healthy(50)
            .with_role(NodeRole::Primary);

        assert_eq!(check.status, NodeHealthStatus::Healthy);
        assert_eq!(check.response_time_ms, Some(50));
        assert_eq!(check.detected_role, Some(NodeRole::Primary));
    }

    #[test]
    fn test_replication_status_lag_acceptable() {
        let mut status = ReplicationStatus::new("node-1");
        status.lag_bytes = Some(1000);

        assert!(status.is_lag_acceptable(1_000_000)); // 1MB
        assert!(!status.is_lag_acceptable(500)); // 500 bytes
    }
}
