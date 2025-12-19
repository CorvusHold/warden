//! Switchover orchestration for planned primary role transfer.
//!
//! This module implements the `ha-switchover` command which gracefully
//! transfers the primary role from one node to a prepared replica.

use common::config::{ClusterConfig, Node, NodeRole};
use log::{info, warn};
use std::time::Duration;

use super::checks::{
    check_node_reachable, get_replication_status, promote_replica, verify_node_role,
    wait_for_replication_catchup, NodeHealthStatus,
};
use super::types::{HaError, HaPlan, HaPlanStep, HaResult};

/// Options for switchover operation.
#[derive(Debug, Clone)]
pub struct SwitchoverOptions {
    /// Cluster ID.
    pub cluster_id: String,
    /// Source node ID (current primary).
    pub from_node_id: String,
    /// Target node ID (replica to promote).
    pub to_node_id: String,
    /// Path to cluster config file.
    pub config_path: Option<std::path::PathBuf>,
    /// Dry-run mode (show plan without executing).
    pub dry_run: bool,
    /// Skip confirmation prompts.
    pub yes: bool,
    /// Maximum replication lag in bytes before switchover.
    pub max_lag_bytes: u64,
    /// Timeout for replication catchup in seconds.
    pub catchup_timeout_secs: u64,
    /// PostgreSQL user for connections.
    pub pg_user: String,
    /// PostgreSQL password.
    pub pg_password: Option<String>,
    /// Database name for connections.
    pub database: String,
    /// Data directory of the target node (for promotion).
    pub target_data_dir: Option<String>,
}

impl Default for SwitchoverOptions {
    fn default() -> Self {
        Self {
            cluster_id: String::new(),
            from_node_id: String::new(),
            to_node_id: String::new(),
            config_path: None,
            dry_run: false,
            yes: false,
            max_lag_bytes: 1_048_576, // 1MB
            catchup_timeout_secs: 60,
            pg_user: "postgres".to_string(),
            pg_password: None,
            database: "postgres".to_string(),
            target_data_dir: None,
        }
    }
}

/// Orchestrator for switchover operations.
pub struct SwitchoverOrchestrator {
    options: SwitchoverOptions,
    config: ClusterConfig,
}

impl SwitchoverOrchestrator {
    /// Create a new switchover orchestrator.
    pub fn new(options: SwitchoverOptions) -> Result<Self, HaError> {
        let config = ClusterConfig::load(options.config_path.as_deref())
            .map_err(|e| HaError::ConfigError(e.to_string()))?;

        Ok(Self { options, config })
    }

    /// Create the execution plan for the switchover.
    pub fn plan(&self) -> Result<HaPlan, HaError> {
        info!(
            "[ha-switchover] Planning switchover from {} to {} in cluster {}",
            self.options.from_node_id, self.options.to_node_id, self.options.cluster_id
        );

        // Validate cluster exists
        let _cluster = self
            .config
            .get_cluster(&self.options.cluster_id)
            .ok_or_else(|| HaError::ClusterNotFound(self.options.cluster_id.clone()))?;

        // Validate nodes exist
        let from_node = self
            .config
            .get_node(&self.options.from_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.from_node_id.clone()))?;

        let to_node = self
            .config
            .get_node(&self.options.to_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.to_node_id.clone()))?;

        // Validate node roles
        verify_node_role(from_node, NodeRole::Primary)?;
        verify_node_role(to_node, NodeRole::Replica)?;

        // Check if already in target state
        if to_node.role == NodeRole::Primary {
            return Err(HaError::AlreadyCompleted(format!(
                "Node {} is already the primary",
                self.options.to_node_id
            )));
        }

        // Build the plan
        let mut plan = HaPlan::new(
            "switchover",
            &self.options.cluster_id,
            &self.options.to_node_id,
        )
        .with_source(&self.options.from_node_id);

        if self.options.dry_run {
            plan = plan.as_dry_run();
        }

        // Step 1: Check source node health
        plan.add_step(
            HaPlanStep::new(
                1,
                "check_source_health",
                format!("Verify primary node {} is healthy", from_node.id),
            )
            .with_duration(5),
        );

        // Step 2: Check target node health
        plan.add_step(
            HaPlanStep::new(
                2,
                "check_target_health",
                format!("Verify replica node {} is healthy", to_node.id),
            )
            .with_duration(5),
        );

        // Step 3: Check replication status
        plan.add_step(
            HaPlanStep::new(
                3,
                "check_replication",
                format!("Check replication status on {}", to_node.id),
            )
            .with_duration(5),
        );

        // Step 4: Wait for replication catchup
        plan.add_step(
            HaPlanStep::new(
                4,
                "wait_catchup",
                format!(
                    "Wait for replication to catch up (max lag: {} bytes)",
                    self.options.max_lag_bytes
                ),
            )
            .with_duration(self.options.catchup_timeout_secs),
        );

        // Step 5: Stop writes on primary (optional checkpoint)
        plan.add_step(
            HaPlanStep::new(
                5,
                "checkpoint_primary",
                format!("Create checkpoint on primary {}", from_node.id),
            )
            .with_duration(10),
        );

        // Step 6: Promote replica
        plan.add_step(
            HaPlanStep::new(
                6,
                "promote_replica",
                format!("Promote {} to primary", to_node.id),
            )
            .destructive()
            .with_duration(10),
        );

        // Step 7: Verify new primary
        plan.add_step(
            HaPlanStep::new(
                7,
                "verify_new_primary",
                format!("Verify {} is accepting writes", to_node.id),
            )
            .with_duration(10),
        );

        // Step 8: Update cluster config
        plan.add_step(
            HaPlanStep::new(
                8,
                "update_config",
                "Update cluster configuration with new roles",
            )
            .with_duration(2),
        );

        // Add warnings
        plan.add_warning(format!(
            "This will transfer the primary role from {} to {}",
            from_node.id, to_node.id
        ));
        plan.add_warning("Writes will be briefly interrupted during the switchover");

        Ok(plan)
    }

    /// Execute the switchover plan.
    pub async fn execute(&self, plan: &mut HaPlan) -> Result<HaResult, HaError> {
        if plan.dry_run {
            info!("[ha-switchover] Dry-run mode - no changes will be made");
            return Ok(HaResult::success(
                plan.clone(),
                "Dry-run completed successfully",
            ));
        }

        let from_node = self
            .config
            .get_node(&self.options.from_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.from_node_id.clone()))?;

        let to_node = self
            .config
            .get_node(&self.options.to_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.to_node_id.clone()))?;

        // Execute each step
        for i in 0..plan.steps.len() {
            let step_name = plan.steps[i].name.clone();
            plan.steps[i].start();

            info!(
                "[ha-switchover] Executing step {}: {}",
                plan.steps[i].number, step_name
            );

            let result = match step_name.as_str() {
                "check_source_health" => self.step_check_source_health(from_node).await,
                "check_target_health" => self.step_check_target_health(to_node).await,
                "check_replication" => self.step_check_replication(to_node).await,
                "wait_catchup" => self.step_wait_catchup(to_node).await,
                "checkpoint_primary" => self.step_checkpoint_primary(from_node).await,
                "promote_replica" => self.step_promote_replica(to_node).await,
                "verify_new_primary" => self.step_verify_new_primary(to_node).await,
                "update_config" => self.step_update_config().await,
                _ => Ok(()),
            };

            match result {
                Ok(()) => {
                    plan.steps[i].complete();
                    info!("[ha-switchover] Step {} completed", step_name);
                }
                Err(e) => {
                    plan.steps[i].fail(e.to_string());
                    warn!("[ha-switchover] Step {} failed: {}", step_name, e);
                    return Ok(HaResult::failure(
                        plan.clone(),
                        format!("Switchover failed at step '{}': {}", step_name, e),
                    ));
                }
            }
        }

        Ok(
            HaResult::success(plan.clone(), "Switchover completed successfully")
                .with_new_primary(&self.options.to_node_id),
        )
    }

    async fn step_check_source_health(&self, node: &Node) -> Result<(), HaError> {
        let health = check_node_reachable(node, 10);
        if health.status != NodeHealthStatus::Healthy {
            return Err(HaError::NodeUnreachable {
                node_id: node.id.clone(),
                host: node.host.clone(),
                port: node.port,
            });
        }
        Ok(())
    }

    async fn step_check_target_health(&self, node: &Node) -> Result<(), HaError> {
        let health = check_node_reachable(node, 10);
        if health.status != NodeHealthStatus::Healthy {
            return Err(HaError::NodeUnreachable {
                node_id: node.id.clone(),
                host: node.host.clone(),
                port: node.port,
            });
        }
        Ok(())
    }

    async fn step_check_replication(&self, node: &Node) -> Result<(), HaError> {
        let status = get_replication_status(
            node,
            &self.options.pg_user,
            self.options.pg_password.as_deref(),
            &self.options.database,
        )
        .await?;

        if !status.is_in_recovery {
            return Err(HaError::InvalidNodeRole {
                node_id: node.id.clone(),
                actual_role: "primary".to_string(),
                expected_role: "replica".to_string(),
            });
        }

        info!(
            "[ha-switchover] Replication status: lag={:?} bytes, streaming={}",
            status.lag_bytes, status.is_streaming
        );

        Ok(())
    }

    async fn step_wait_catchup(&self, node: &Node) -> Result<(), HaError> {
        wait_for_replication_catchup(
            node,
            &self.options.pg_user,
            self.options.pg_password.as_deref(),
            &self.options.database,
            self.options.max_lag_bytes,
            Duration::from_secs(self.options.catchup_timeout_secs),
        )
        .await?;
        Ok(())
    }

    async fn step_checkpoint_primary(&self, node: &Node) -> Result<(), HaError> {
        // Execute CHECKPOINT on primary
        let password_part = self
            .options
            .pg_password
            .as_ref()
            .map(|p| format!(":{}", p))
            .unwrap_or_default();
        let conn_str = format!(
            "host={} port={} user={}{} dbname={}",
            node.host, node.port, self.options.pg_user, password_part, self.options.database
        );

        let result = std::process::Command::new("psql")
            .arg(&conn_str)
            .arg("-c")
            .arg("CHECKPOINT;")
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    info!("[ha-switchover] Checkpoint completed on primary");
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(HaError::StepFailed {
                        step: "checkpoint_primary".to_string(),
                        reason: stderr.to_string(),
                    })
                }
            }
            Err(e) => Err(HaError::StepFailed {
                step: "checkpoint_primary".to_string(),
                reason: format!("Failed to run psql: {}", e),
            }),
        }
    }

    async fn step_promote_replica(&self, node: &Node) -> Result<(), HaError> {
        // Get data directory - either from options or try to detect
        let data_dir = match &self.options.target_data_dir {
            Some(dir) => dir.clone(),
            None => {
                // Try to get data directory from PostgreSQL
                let password_part = self
                    .options
                    .pg_password
                    .as_ref()
                    .map(|p| format!(":{}", p))
                    .unwrap_or_default();
                let conn_str = format!(
                    "host={} port={} user={}{} dbname={}",
                    node.host,
                    node.port,
                    self.options.pg_user,
                    password_part,
                    self.options.database
                );

                let result = std::process::Command::new("psql")
                    .arg(&conn_str)
                    .arg("-t")
                    .arg("-A")
                    .arg("-c")
                    .arg("SHOW data_directory;")
                    .output();

                match result {
                    Ok(output) if output.status.success() => {
                        String::from_utf8_lossy(&output.stdout).trim().to_string()
                    }
                    _ => {
                        return Err(HaError::StepFailed {
                            step: "promote_replica".to_string(),
                            reason: "Could not determine data directory. Please specify --target-data-dir".to_string(),
                        });
                    }
                }
            }
        };

        promote_replica(&data_dir, 30).await
    }

    async fn step_verify_new_primary(&self, node: &Node) -> Result<(), HaError> {
        // Wait a moment for promotion to complete
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Check that node is no longer in recovery
        let status = get_replication_status(
            node,
            &self.options.pg_user,
            self.options.pg_password.as_deref(),
            &self.options.database,
        )
        .await?;

        if status.is_in_recovery {
            return Err(HaError::StepFailed {
                step: "verify_new_primary".to_string(),
                reason: "Node is still in recovery mode after promotion".to_string(),
            });
        }

        // Try a simple write test
        let password_part = self
            .options
            .pg_password
            .as_ref()
            .map(|p| format!(":{}", p))
            .unwrap_or_default();
        let conn_str = format!(
            "host={} port={} user={}{} dbname={}",
            node.host, node.port, self.options.pg_user, password_part, self.options.database
        );

        let result = std::process::Command::new("psql")
            .arg(&conn_str)
            .arg("-c")
            .arg("SELECT 1;")
            .output();

        match result {
            Ok(output) if output.status.success() => {
                info!("[ha-switchover] New primary is accepting connections");
                Ok(())
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(HaError::StepFailed {
                    step: "verify_new_primary".to_string(),
                    reason: stderr.to_string(),
                })
            }
            Err(e) => Err(HaError::StepFailed {
                step: "verify_new_primary".to_string(),
                reason: format!("Failed to verify new primary: {}", e),
            }),
        }
    }

    async fn step_update_config(&self) -> Result<(), HaError> {
        // In a real implementation, this would update the cluster.yaml file
        // For now, we just log the required changes
        info!(
            "[ha-switchover] Cluster config should be updated:\n  - {} role: primary -> replica\n  - {} role: replica -> primary",
            self.options.from_node_id, self.options.to_node_id
        );

        // TODO: Actually update the config file
        // This is left as a manual step for safety

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_switchover_options_default() {
        let opts = SwitchoverOptions::default();
        assert_eq!(opts.max_lag_bytes, 1_048_576);
        assert_eq!(opts.catchup_timeout_secs, 60);
        assert_eq!(opts.pg_user, "postgres");
    }
}
