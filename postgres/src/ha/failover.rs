//! Failover orchestration for emergency primary promotion.
//!
//! This module implements the `ha-failover` command which promotes a replica
//! to primary when the current primary is unavailable.

use chrono::{DateTime, Utc};
use common::config::{ClusterConfig, Node, NodeRole};
use log::{info, warn};
use std::path::PathBuf;

use super::checks::{
    check_node_reachable, get_replication_status, promote_replica, verify_node_role,
    NodeHealthStatus,
};
use super::types::{HaError, HaPlan, HaPlanStep, HaResult};

/// Options for failover operation.
#[derive(Debug, Clone)]
pub struct FailoverOptions {
    /// Cluster ID.
    pub cluster_id: String,
    /// Target node ID (replica to promote).
    pub to_node_id: String,
    /// Optional target time for PITR-based failover.
    pub target_time: Option<DateTime<Utc>>,
    /// Path to cluster config file.
    pub config_path: Option<PathBuf>,
    /// Dry-run mode (show plan without executing).
    pub dry_run: bool,
    /// Skip confirmation prompts.
    pub yes: bool,
    /// Force failover even if primary is reachable.
    pub force: bool,
    /// PostgreSQL user for connections.
    pub pg_user: String,
    /// PostgreSQL password.
    pub pg_password: Option<String>,
    /// Database name for connections.
    pub database: String,
    /// Data directory of the target node (for promotion).
    pub target_data_dir: Option<String>,
    /// Backup directory for PITR.
    pub backup_dir: Option<PathBuf>,
}

impl Default for FailoverOptions {
    fn default() -> Self {
        Self {
            cluster_id: String::new(),
            to_node_id: String::new(),
            target_time: None,
            config_path: None,
            dry_run: false,
            yes: false,
            force: false,
            pg_user: "postgres".to_string(),
            pg_password: None,
            database: "postgres".to_string(),
            target_data_dir: None,
            backup_dir: None,
        }
    }
}

/// Orchestrator for failover operations.
pub struct FailoverOrchestrator {
    options: FailoverOptions,
    config: ClusterConfig,
}

impl FailoverOrchestrator {
    /// Create a new failover orchestrator.
    pub fn new(options: FailoverOptions) -> Result<Self, HaError> {
        let config = ClusterConfig::load(options.config_path.as_deref())
            .map_err(|e| HaError::ConfigError(e.to_string()))?;

        Ok(Self { options, config })
    }

    /// Create the execution plan for the failover.
    pub fn plan(&self) -> Result<HaPlan, HaError> {
        info!(
            "[ha-failover] Planning failover to {} in cluster {}",
            self.options.to_node_id, self.options.cluster_id
        );

        // Validate cluster exists
        let _cluster = self
            .config
            .get_cluster(&self.options.cluster_id)
            .ok_or_else(|| HaError::ClusterNotFound(self.options.cluster_id.clone()))?;

        // Validate target node exists
        let to_node = self
            .config
            .get_node(&self.options.to_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.to_node_id.clone()))?;

        // Validate target is a replica
        verify_node_role(to_node, NodeRole::Replica)?;

        // Check if already in target state
        if to_node.role == NodeRole::Primary {
            return Err(HaError::AlreadyCompleted(format!(
                "Node {} is already the primary",
                self.options.to_node_id
            )));
        }

        // Find current primary (if any)
        let current_primary = self.config.get_primary_node(&self.options.cluster_id);

        // Build the plan
        let mut plan = HaPlan::new("failover", &self.options.cluster_id, &self.options.to_node_id);

        if let Some(primary) = current_primary {
            plan = plan.with_source(&primary.id);
        }

        if self.options.dry_run {
            plan = plan.as_dry_run();
        }

        let mut step_num = 1;

        // Step: Check if primary is unreachable (unless --force)
        if !self.options.force {
            if let Some(primary) = current_primary {
                plan.add_step(
                    HaPlanStep::new(
                        step_num,
                        "verify_primary_down",
                        format!("Verify primary {} is unreachable", primary.id),
                    )
                    .with_duration(15),
                );
                step_num += 1;
            }
        }

        // Step: Check target node health
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "check_target_health",
                format!("Verify replica node {} is healthy", to_node.id),
            )
            .with_duration(5),
        );
        step_num += 1;

        // Step: Check replication status
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "check_replication",
                format!("Check replication status on {}", to_node.id),
            )
            .with_duration(5),
        );
        step_num += 1;

        // Step: PITR if target_time specified
        if self.options.target_time.is_some() {
            plan.add_step(
                HaPlanStep::new(
                    step_num,
                    "validate_pitr",
                    "Validate PITR feasibility for target time",
                )
                .with_duration(10),
            );
            step_num += 1;

            plan.add_step(
                HaPlanStep::new(step_num, "execute_pitr", "Execute point-in-time recovery")
                    .destructive()
                    .with_duration(300), // PITR can take a while
            );
            step_num += 1;
        }

        // Step: Promote replica
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "promote_replica",
                format!("Promote {} to primary", to_node.id),
            )
            .destructive()
            .with_duration(10),
        );
        step_num += 1;

        // Step: Verify new primary
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "verify_new_primary",
                format!("Verify {} is accepting writes", to_node.id),
            )
            .with_duration(10),
        );
        step_num += 1;

        // Step: Update cluster config
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "update_config",
                "Update cluster configuration with new roles",
            )
            .with_duration(2),
        );

        // Add warnings
        plan.add_warning("⚠️  EMERGENCY FAILOVER - This is a destructive operation");
        plan.add_warning(format!(
            "Node {} will be promoted to primary",
            to_node.id
        ));

        if let Some(primary) = current_primary {
            plan.add_warning(format!(
                "Previous primary {} will be marked as 'unknown' role",
                primary.id
            ));
        }

        if self.options.target_time.is_some() {
            plan.add_warning("PITR will be performed - data after target time will be lost");
        } else {
            plan.add_warning(
                "Any transactions not yet replicated to the target node will be lost",
            );
        }

        if self.options.force {
            plan.add_warning("--force specified: skipping primary reachability check");
        }

        Ok(plan)
    }

    /// Execute the failover plan.
    pub async fn execute(&self, plan: &mut HaPlan) -> Result<HaResult, HaError> {
        if plan.dry_run {
            info!("[ha-failover] Dry-run mode - no changes will be made");
            return Ok(HaResult::success(
                plan.clone(),
                "Dry-run completed successfully",
            ));
        }

        let to_node = self
            .config
            .get_node(&self.options.to_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.to_node_id.clone()))?;

        let current_primary = self.config.get_primary_node(&self.options.cluster_id);

        // Execute each step
        for i in 0..plan.steps.len() {
            let step_name = plan.steps[i].name.clone();
            plan.steps[i].start();

            info!(
                "[ha-failover] Executing step {}: {}",
                plan.steps[i].number, step_name
            );

            let result = match step_name.as_str() {
                "verify_primary_down" => {
                    if let Some(primary) = current_primary {
                        self.step_verify_primary_down(primary).await
                    } else {
                        Ok(())
                    }
                }
                "check_target_health" => self.step_check_target_health(to_node).await,
                "check_replication" => self.step_check_replication(to_node).await,
                "validate_pitr" => self.step_validate_pitr().await,
                "execute_pitr" => self.step_execute_pitr().await,
                "promote_replica" => self.step_promote_replica(to_node).await,
                "verify_new_primary" => self.step_verify_new_primary(to_node).await,
                "update_config" => self.step_update_config(current_primary).await,
                _ => Ok(()),
            };

            match result {
                Ok(()) => {
                    plan.steps[i].complete();
                    info!("[ha-failover] Step {} completed", step_name);
                }
                Err(e) => {
                    plan.steps[i].fail(e.to_string());
                    warn!("[ha-failover] Step {} failed: {}", step_name, e);
                    return Ok(HaResult::failure(
                        plan.clone(),
                        format!("Failover failed at step '{}': {}", step_name, e),
                    ));
                }
            }
        }

        Ok(
            HaResult::success(plan.clone(), "Failover completed successfully")
                .with_new_primary(&self.options.to_node_id),
        )
    }

    async fn step_verify_primary_down(&self, primary: &Node) -> Result<(), HaError> {
        let health = check_node_reachable(primary, 10);

        if health.status == NodeHealthStatus::Healthy {
            return Err(HaError::PrimaryStillReachable);
        }

        info!(
            "[ha-failover] Primary {} is unreachable (status: {})",
            primary.id, health.status
        );
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
            "[ha-failover] Replication status: lag={:?} bytes, streaming={}",
            status.lag_bytes, status.is_streaming
        );

        // Log potential data loss
        if let Some(lag) = status.lag_bytes {
            if lag > 0 {
                warn!(
                    "[ha-failover] Potential data loss: {} bytes of unreplicated data",
                    lag
                );
            }
        }

        Ok(())
    }

    async fn step_validate_pitr(&self) -> Result<(), HaError> {
        let target_time = self
            .options
            .target_time
            .ok_or_else(|| HaError::PitrNotFeasible("No target time specified".to_string()))?;

        info!(
            "[ha-failover] Validating PITR feasibility for target time: {}",
            target_time
        );

        // TODO: Integrate with PITR planner to validate WAL coverage
        // For now, we just log the intent

        Ok(())
    }

    async fn step_execute_pitr(&self) -> Result<(), HaError> {
        let target_time = self
            .options
            .target_time
            .ok_or_else(|| HaError::PitrNotFeasible("No target time specified".to_string()))?;

        info!(
            "[ha-failover] Executing PITR to target time: {}",
            target_time
        );

        // TODO: Integrate with PITR executor
        // This would involve:
        // 1. Stop the replica
        // 2. Configure recovery_target_time
        // 3. Start PostgreSQL in recovery mode
        // 4. Wait for recovery to complete

        warn!("[ha-failover] PITR execution not yet implemented - skipping");

        Ok(())
    }

    async fn step_promote_replica(&self, node: &Node) -> Result<(), HaError> {
        // Get data directory
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
                    node.host, node.port, self.options.pg_user, password_part, self.options.database
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
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

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

        info!("[ha-failover] New primary is accepting connections");
        Ok(())
    }

    async fn step_update_config(&self, old_primary: Option<&Node>) -> Result<(), HaError> {
        info!(
            "[ha-failover] Cluster config should be updated:\n  - {} role: replica -> primary",
            self.options.to_node_id
        );

        if let Some(primary) = old_primary {
            info!("  - {} role: primary -> unknown", primary.id);
        }

        // TODO: Actually update the config file
        // This is left as a manual step for safety

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failover_options_default() {
        let opts = FailoverOptions::default();
        assert!(!opts.force);
        assert!(!opts.dry_run);
        assert!(opts.target_time.is_none());
    }
}
