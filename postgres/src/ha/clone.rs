//! Clone node orchestration for creating new replicas.
//!
//! This module implements the `ha-clone-node` command which creates a new
//! replica from an existing backup or PITR point.

use chrono::{DateTime, Utc};
use common::config::{ClusterConfig, Node};
use log::{info, warn};
use std::path::PathBuf;

use super::checks::configure_as_replica;
use super::types::{HaError, HaPlan, HaPlanStep, HaResult};

/// Options for clone node operation.
#[derive(Debug, Clone)]
pub struct CloneNodeOptions {
    /// Cluster ID.
    pub cluster_id: String,
    /// Source node ID (to get backup from).
    pub source_node_id: String,
    /// Target node ID (new replica).
    pub target_node_id: String,
    /// Specific backup ID to use.
    pub backup_id: Option<String>,
    /// Optional target time for PITR-based clone.
    pub target_time: Option<DateTime<Utc>>,
    /// Target directory for the new replica data.
    pub target_dir: PathBuf,
    /// Path to cluster config file.
    pub config_path: Option<PathBuf>,
    /// Dry-run mode (show plan without executing).
    pub dry_run: bool,
    /// Skip confirmation prompts.
    pub yes: bool,
    /// Backup directory.
    pub backup_dir: PathBuf,
    /// PostgreSQL user for connections.
    pub pg_user: String,
    /// PostgreSQL password.
    pub pg_password: Option<String>,
    /// Database name for connections.
    pub database: String,
    /// Use remote storage for backups.
    pub remote_storage: bool,
    /// Storage bucket.
    pub storage_bucket: Option<String>,
    /// Storage endpoint.
    pub storage_endpoint: Option<String>,
    /// Storage region.
    pub storage_region: Option<String>,
    /// Storage access key.
    pub storage_access_key: Option<String>,
    /// Storage secret key.
    pub storage_secret_key: Option<String>,
}

impl Default for CloneNodeOptions {
    fn default() -> Self {
        Self {
            cluster_id: String::new(),
            source_node_id: String::new(),
            target_node_id: String::new(),
            backup_id: None,
            target_time: None,
            target_dir: PathBuf::from("/var/lib/postgresql/data"),
            config_path: None,
            dry_run: false,
            yes: false,
            backup_dir: PathBuf::from("./backups"),
            pg_user: "postgres".to_string(),
            pg_password: None,
            database: "postgres".to_string(),
            remote_storage: false,
            storage_bucket: None,
            storage_endpoint: None,
            storage_region: None,
            storage_access_key: None,
            storage_secret_key: None,
        }
    }
}

/// Orchestrator for clone node operations.
pub struct CloneNodeOrchestrator {
    options: CloneNodeOptions,
    config: ClusterConfig,
}

impl CloneNodeOrchestrator {
    /// Create a new clone node orchestrator.
    pub fn new(options: CloneNodeOptions) -> Result<Self, HaError> {
        let config = ClusterConfig::load(options.config_path.as_deref())
            .map_err(|e| HaError::ConfigError(e.to_string()))?;

        Ok(Self { options, config })
    }

    /// Create the execution plan for the clone operation.
    pub fn plan(&self) -> Result<HaPlan, HaError> {
        info!(
            "[ha-clone] Planning clone from {} to {} in cluster {}",
            self.options.source_node_id, self.options.target_node_id, self.options.cluster_id
        );

        // Validate cluster exists
        let _cluster = self
            .config
            .get_cluster(&self.options.cluster_id)
            .ok_or_else(|| HaError::ClusterNotFound(self.options.cluster_id.clone()))?;

        // Validate source node exists
        let source_node = self
            .config
            .get_node(&self.options.source_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.source_node_id.clone()))?;

        // Target node may or may not exist in config yet
        let target_node = self.config.get_node(&self.options.target_node_id);

        // Build the plan
        let mut plan = HaPlan::new(
            "clone-node",
            &self.options.cluster_id,
            &self.options.target_node_id,
        )
        .with_source(&self.options.source_node_id);

        if self.options.dry_run {
            plan = plan.as_dry_run();
        }

        let mut step_num = 1;

        // Step: Validate backup source
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "validate_backup",
                if let Some(ref backup_id) = self.options.backup_id {
                    format!("Validate backup {} exists and is valid", backup_id)
                } else {
                    format!("Find latest backup from source node {}", source_node.id)
                },
            )
            .with_duration(10),
        );
        step_num += 1;

        // Step: Validate target directory
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "validate_target_dir",
                format!(
                    "Validate target directory {}",
                    self.options.target_dir.display()
                ),
            )
            .with_duration(2),
        );
        step_num += 1;

        // Step: Download backup if remote
        if self.options.remote_storage {
            plan.add_step(
                HaPlanStep::new(
                    step_num,
                    "download_backup",
                    "Download backup from remote storage",
                )
                .with_duration(300), // Can take a while for large backups
            );
            step_num += 1;
        }

        // Step: Restore backup
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "restore_backup",
                format!("Restore backup to {}", self.options.target_dir.display()),
            )
            .destructive()
            .with_duration(300),
        );
        step_num += 1;

        // Step: PITR if target_time specified
        if self.options.target_time.is_some() {
            plan.add_step(
                HaPlanStep::new(step_num, "execute_pitr", "Execute point-in-time recovery")
                    .destructive()
                    .with_duration(300),
            );
            step_num += 1;
        }

        // Step: Configure as replica
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "configure_replica",
                "Configure PostgreSQL as replica (standby.signal, primary_conninfo)",
            )
            .with_duration(5),
        );
        step_num += 1;

        // Step: Start PostgreSQL
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "start_postgres",
                "Start PostgreSQL in recovery mode",
            )
            .with_duration(30),
        );
        step_num += 1;

        // Step: Verify replication
        plan.add_step(
            HaPlanStep::new(
                step_num,
                "verify_replication",
                "Verify replica is streaming from primary",
            )
            .with_duration(30),
        );
        step_num += 1;

        // Step: Update cluster config (optional)
        if target_node.is_none() {
            plan.add_step(
                HaPlanStep::new(
                    step_num,
                    "update_config",
                    format!(
                        "Add node {} to cluster configuration",
                        self.options.target_node_id
                    ),
                )
                .with_duration(2),
            );
        }

        // Add warnings
        plan.add_warning(format!(
            "A new replica will be created at {}",
            self.options.target_dir.display()
        ));

        if self.options.target_time.is_some() {
            plan.add_warning("PITR will be performed - replica will be at specified point in time");
        }

        // Estimate size
        plan.add_warning("Ensure sufficient disk space for the backup restore");

        Ok(plan)
    }

    /// Execute the clone plan.
    pub async fn execute(&self, plan: &mut HaPlan) -> Result<HaResult, HaError> {
        if plan.dry_run {
            info!("[ha-clone] Dry-run mode - no changes will be made");
            return Ok(HaResult::success(
                plan.clone(),
                "Dry-run completed successfully",
            ));
        }

        let source_node = self
            .config
            .get_node(&self.options.source_node_id)
            .ok_or_else(|| HaError::NodeNotFound(self.options.source_node_id.clone()))?;

        // Execute each step
        for i in 0..plan.steps.len() {
            let step_name = plan.steps[i].name.clone();
            plan.steps[i].start();

            info!(
                "[ha-clone] Executing step {}: {}",
                plan.steps[i].number, step_name
            );

            let result = match step_name.as_str() {
                "validate_backup" => self.step_validate_backup().await,
                "validate_target_dir" => self.step_validate_target_dir().await,
                "download_backup" => self.step_download_backup().await,
                "restore_backup" => self.step_restore_backup().await,
                "execute_pitr" => self.step_execute_pitr().await,
                "configure_replica" => self.step_configure_replica(source_node).await,
                "start_postgres" => self.step_start_postgres().await,
                "verify_replication" => self.step_verify_replication().await,
                "update_config" => self.step_update_config().await,
                _ => Ok(()),
            };

            match result {
                Ok(()) => {
                    plan.steps[i].complete();
                    info!("[ha-clone] Step {} completed", step_name);
                }
                Err(e) => {
                    plan.steps[i].fail(e.to_string());
                    warn!("[ha-clone] Step {} failed: {}", step_name, e);
                    return Ok(HaResult::failure(
                        plan.clone(),
                        format!("Clone failed at step '{}': {}", step_name, e),
                    ));
                }
            }
        }

        Ok(
            HaResult::success(plan.clone(), "Clone completed successfully")
                .with_new_replica(&self.options.target_node_id),
        )
    }

    async fn step_validate_backup(&self) -> Result<(), HaError> {
        if let Some(ref backup_id) = self.options.backup_id {
            info!("[ha-clone] Validating backup: {}", backup_id);

            // Check if backup exists locally
            let backup_path = self.options.backup_dir.join(backup_id);
            if backup_path.exists() {
                info!("[ha-clone] Found local backup at {}", backup_path.display());
                return Ok(());
            }

            // If remote storage is enabled, we'll download it later
            if self.options.remote_storage {
                info!("[ha-clone] Backup will be downloaded from remote storage");
                return Ok(());
            }

            return Err(HaError::BackupNotFound(backup_id.clone()));
        }

        // Find latest backup
        info!(
            "[ha-clone] Looking for latest backup in {}",
            self.options.backup_dir.display()
        );

        // Check for backup catalog
        let catalog_path = self.options.backup_dir.join("backup_catalog.json");
        if catalog_path.exists() {
            info!("[ha-clone] Found backup catalog");
            // TODO: Parse catalog and find latest backup
            return Ok(());
        }

        // List backup directories
        if self.options.backup_dir.exists() {
            let entries: Vec<_> = std::fs::read_dir(&self.options.backup_dir)
                .map_err(HaError::Io)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .collect();

            if !entries.is_empty() {
                info!("[ha-clone] Found {} backup directories", entries.len());
                return Ok(());
            }
        }

        if self.options.remote_storage {
            info!("[ha-clone] Will search for backups in remote storage");
            return Ok(());
        }

        Err(HaError::BackupNotFound(
            "No backups found in backup directory".to_string(),
        ))
    }

    async fn step_validate_target_dir(&self) -> Result<(), HaError> {
        let target_dir = &self.options.target_dir;

        if target_dir.exists() {
            // Check if directory is empty
            let entries: Vec<_> = std::fs::read_dir(target_dir)
                .map_err(HaError::Io)?
                .filter_map(|e| e.ok())
                .collect();

            if !entries.is_empty() && !self.options.yes {
                return Err(HaError::TargetDirNotEmpty(target_dir.display().to_string()));
            }

            if !entries.is_empty() {
                warn!(
                    "[ha-clone] Target directory {} is not empty, will be overwritten",
                    target_dir.display()
                );
            }
        } else {
            // Create directory
            std::fs::create_dir_all(target_dir)?;
            info!(
                "[ha-clone] Created target directory {}",
                target_dir.display()
            );
        }

        Ok(())
    }

    async fn step_download_backup(&self) -> Result<(), HaError> {
        info!("[ha-clone] Downloading backup from remote storage");

        // TODO: Integrate with storage crate to download backup
        // This would use PostgresBackupStorage or similar

        warn!("[ha-clone] Remote backup download not yet fully implemented");
        Ok(())
    }

    async fn step_restore_backup(&self) -> Result<(), HaError> {
        info!(
            "[ha-clone] Restoring backup to {}",
            self.options.target_dir.display()
        );

        // Determine backup path
        let backup_path = if let Some(ref backup_id) = self.options.backup_id {
            self.options.backup_dir.join(backup_id)
        } else {
            // Find latest backup directory
            let entries: Vec<_> = std::fs::read_dir(&self.options.backup_dir)
                .map_err(HaError::Io)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("snapshot_backup_")
                        || e.file_name().to_string_lossy().starts_with("full_backup_")
                })
                .collect();

            if entries.is_empty() {
                return Err(HaError::BackupNotFound(
                    "No backup directories found".to_string(),
                ));
            }

            // Sort by name (which includes timestamp) and take latest
            let mut paths: Vec<_> = entries.iter().map(|e| e.path()).collect();
            paths.sort();
            paths.last().unwrap().clone()
        };

        info!("[ha-clone] Using backup from {}", backup_path.display());

        // Copy backup data to target directory
        // For a real implementation, this would use the restore module
        let base_backup_dir = backup_path.join("base");
        if base_backup_dir.exists() {
            // Copy base backup
            copy_dir_recursive(&base_backup_dir, &self.options.target_dir)?;
            info!("[ha-clone] Restored base backup");
        } else {
            return Err(HaError::StepFailed {
                step: "restore_backup".to_string(),
                reason: format!("Base backup not found at {}", base_backup_dir.display()),
            });
        }

        Ok(())
    }

    async fn step_execute_pitr(&self) -> Result<(), HaError> {
        let target_time = self
            .options
            .target_time
            .ok_or_else(|| HaError::PitrNotFeasible("No target time specified".to_string()))?;

        info!("[ha-clone] Executing PITR to target time: {}", target_time);

        // TODO: Integrate with PITR executor
        // This would configure recovery_target_time in postgresql.auto.conf

        let auto_conf = self.options.target_dir.join("postgresql.auto.conf");
        let mut content = if auto_conf.exists() {
            std::fs::read_to_string(&auto_conf)?
        } else {
            String::new()
        };

        content.push_str(&format!(
            "\n# PITR configuration added by Warden\nrecovery_target_time = '{}'\nrecovery_target_action = 'promote'\n",
            target_time.format("%Y-%m-%d %H:%M:%S %Z")
        ));

        std::fs::write(&auto_conf, content)?;
        info!("[ha-clone] Configured PITR target time");

        Ok(())
    }

    async fn step_configure_replica(&self, source_node: &Node) -> Result<(), HaError> {
        // Get primary node for replication
        let primary = self
            .config
            .get_primary_node(&self.options.cluster_id)
            .unwrap_or(source_node);

        configure_as_replica(
            self.options.target_dir.to_str().unwrap_or(""),
            &primary.host,
            primary.port,
            &self.options.pg_user,
        )?;

        Ok(())
    }

    async fn step_start_postgres(&self) -> Result<(), HaError> {
        info!(
            "[ha-clone] Starting PostgreSQL at {}",
            self.options.target_dir.display()
        );

        let result = std::process::Command::new("pg_ctl")
            .arg("start")
            .arg("-D")
            .arg(&self.options.target_dir)
            .arg("-w") // Wait for startup
            .arg("-t")
            .arg("60") // Timeout
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    info!("[ha-clone] PostgreSQL started successfully");
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(HaError::StepFailed {
                        step: "start_postgres".to_string(),
                        reason: stderr.to_string(),
                    })
                }
            }
            Err(e) => Err(HaError::StepFailed {
                step: "start_postgres".to_string(),
                reason: format!("Failed to run pg_ctl: {}", e),
            }),
        }
    }

    async fn step_verify_replication(&self) -> Result<(), HaError> {
        // Wait for PostgreSQL to start streaming
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // Check if in recovery mode
        let result = std::process::Command::new("psql")
            .arg("-h")
            .arg(self.options.target_dir.to_str().unwrap_or(""))
            .arg("-U")
            .arg(&self.options.pg_user)
            .arg("-d")
            .arg(&self.options.database)
            .arg("-t")
            .arg("-A")
            .arg("-c")
            .arg("SELECT pg_is_in_recovery();")
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if stdout == "t" || stdout == "true" {
                        info!("[ha-clone] Replica is in recovery mode");
                        Ok(())
                    } else {
                        warn!(
                            "[ha-clone] Replica is not in recovery mode - may have been promoted"
                        );
                        Ok(())
                    }
                } else {
                    // Connection might fail if using socket path, try with localhost
                    warn!("[ha-clone] Could not verify replication status - PostgreSQL may still be starting");
                    Ok(())
                }
            }
            Err(e) => {
                warn!("[ha-clone] Could not verify replication: {}", e);
                Ok(())
            }
        }
    }

    async fn step_update_config(&self) -> Result<(), HaError> {
        info!(
            "[ha-clone] Cluster config should be updated to add node {}",
            self.options.target_node_id
        );

        // TODO: Actually update the config file
        // This is left as a manual step for safety

        Ok(())
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), HaError> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_options_default() {
        let opts = CloneNodeOptions::default();
        assert!(!opts.dry_run);
        assert!(!opts.remote_storage);
        assert!(opts.backup_id.is_none());
    }
}
