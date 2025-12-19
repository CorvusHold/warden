//! Local scheduler engine for automated backup and retention tasks.
//!
//! This module implements a scheduler that reads schedule configuration and
//! triggers backup/retention operations at the configured times.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use common::config::WardenConfig;
use common::schedule::{
    BackupSchedule, BackupTarget, BackupType, ParsedSchedule, RetentionSchedule, ScheduleConfig,
    ScheduleType, ScheduledRun, StorageProfile,
};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

/// Result of a scheduled task execution.
#[derive(Debug, Clone)]
pub struct TaskResult {
    pub schedule_id: String,
    pub schedule_type: ScheduleType,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub success: bool,
    pub message: Option<String>,
    pub backup_id: Option<String>,
}

/// Event emitted by the scheduler.
#[derive(Debug, Clone)]
pub enum SchedulerEvent {
    /// A scheduled task started
    TaskStarted {
        schedule_id: String,
        schedule_type: ScheduleType,
        started_at: DateTime<Utc>,
    },
    /// A scheduled task completed
    TaskCompleted(TaskResult),
    /// Scheduler encountered an error
    Error {
        schedule_id: Option<String>,
        message: String,
    },
}

/// Options for the scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerOptions {
    /// How often to check for due schedules (in seconds)
    pub check_interval_secs: u64,
    /// Tolerance for schedule matching (in seconds)
    pub tolerance_secs: i64,
    /// Whether to run in dry-run mode (log but don't execute)
    pub dry_run: bool,
    /// Default backup directory
    pub default_backup_dir: PathBuf,
}

impl Default for SchedulerOptions {
    fn default() -> Self {
        Self {
            check_interval_secs: 60, // Check every minute
            tolerance_secs: 30,      // 30 second tolerance
            dry_run: false,
            default_backup_dir: PathBuf::from("./backups"),
        }
    }
}

/// The scheduler engine.
pub struct Scheduler {
    config: Arc<Mutex<WardenConfig>>,
    options: SchedulerOptions,
    /// Track last run times to avoid duplicate executions
    last_runs: HashMap<String, DateTime<Utc>>,
    /// Channel for emitting events
    event_tx: Option<mpsc::Sender<SchedulerEvent>>,
    /// Shutdown signal
    shutdown: bool,
}

impl Scheduler {
    /// Create a new scheduler with the given configuration.
    pub fn new(config: Arc<Mutex<WardenConfig>>, options: SchedulerOptions) -> Self {
        Self {
            config,
            options,
            last_runs: HashMap::new(),
            event_tx: None,
            shutdown: false,
        }
    }

    /// Set the event channel for receiving scheduler events.
    pub fn with_event_channel(mut self, tx: mpsc::Sender<SchedulerEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Get the schedule configuration from the current config.
    fn get_schedule_config(&self) -> Option<ScheduleConfig> {
        let config = self.config.lock().unwrap();
        config.schedules.clone()
    }

    /// Run the scheduler loop.
    ///
    /// This method runs indefinitely, checking for due schedules at the configured interval.
    pub async fn run(&mut self) -> Result<()> {
        info!(
            "Scheduler starting with check interval of {} seconds",
            self.options.check_interval_secs
        );

        let mut check_interval = interval(Duration::from_secs(self.options.check_interval_secs));

        loop {
            check_interval.tick().await;

            if self.shutdown {
                info!("Scheduler shutting down");
                break;
            }

            let now = Utc::now();
            debug!("Scheduler tick at {}", now);

            if let Some(schedule_config) = self.get_schedule_config() {
                // Check backup schedules
                for schedule in schedule_config.enabled_backup_schedules() {
                    if self.should_run_schedule(&schedule.id, &schedule.cron, now) {
                        self.execute_backup_schedule(schedule, &schedule_config)
                            .await;
                    }
                }

                // Check retention schedules
                for schedule in schedule_config.enabled_retention_schedules() {
                    if self.should_run_schedule(&schedule.id, &schedule.cron, now) {
                        self.execute_retention_schedule(schedule, &schedule_config)
                            .await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a schedule should run now.
    fn should_run_schedule(&mut self, id: &str, cron_expr: &str, now: DateTime<Utc>) -> bool {
        // Parse the schedule
        let parsed = match ParsedSchedule::new(id.to_string(), cron_expr) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to parse schedule '{}': {}", id, e);
                return false;
            }
        };

        // Check if we should run based on cron expression
        if !parsed.should_run_at(now, self.options.tolerance_secs) {
            return false;
        }

        // Check if we already ran recently (prevent duplicate runs)
        if let Some(last_run) = self.last_runs.get(id) {
            let since_last = (now - *last_run).num_seconds();
            // Don't run if we ran within the last check interval
            if since_last < self.options.check_interval_secs as i64 {
                debug!("Skipping schedule '{}': ran {} seconds ago", id, since_last);
                return false;
            }
        }

        true
    }

    /// Execute a backup schedule.
    async fn execute_backup_schedule(
        &mut self,
        schedule: &BackupSchedule,
        config: &ScheduleConfig,
    ) {
        let started_at = Utc::now();
        info!(
            "Executing backup schedule '{}' (type: {})",
            schedule.id, schedule.backup_type
        );

        // Record that we're running this schedule
        self.last_runs.insert(schedule.id.clone(), started_at);

        // Emit start event
        if let Some(tx) = &self.event_tx {
            let _ = tx
                .send(SchedulerEvent::TaskStarted {
                    schedule_id: schedule.id.clone(),
                    schedule_type: ScheduleType::Backup,
                    started_at,
                })
                .await;
        }

        if self.options.dry_run {
            info!(
                "[DRY-RUN] Would execute backup schedule '{}' with target: {:?}",
                schedule.id, schedule.target
            );
            // Emit TaskCompleted event for dry-run to maintain event consistency
            if let Some(tx) = &self.event_tx {
                let _ = tx
                    .send(SchedulerEvent::TaskCompleted(TaskResult {
                        schedule_id: schedule.id.clone(),
                        schedule_type: ScheduleType::Backup,
                        started_at,
                        completed_at: Utc::now(),
                        success: true,
                        message: Some("Dry-run completed".to_string()),
                        backup_id: None,
                    }))
                    .await;
            }
            return;
        }

        // Get storage profile if specified
        let storage_profile = schedule
            .storage_profile
            .as_ref()
            .and_then(|name| config.get_storage_profile(name));

        // Determine backup directory
        let backup_dir = schedule
            .backup_dir
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| config.default_backup_dir.as_ref().map(PathBuf::from))
            .unwrap_or_else(|| self.options.default_backup_dir.clone());

        // Execute the backup based on target type
        let result = match &schedule.target {
            BackupTarget::Database {
                host,
                port,
                database,
                user,
            } => {
                self.execute_database_backup(
                    &schedule.id,
                    host,
                    port.unwrap_or(5432),
                    database,
                    user.as_deref().unwrap_or("postgres"),
                    &schedule.backup_type,
                    &backup_dir,
                    storage_profile,
                    &schedule.labels,
                )
                .await
            }
            BackupTarget::Cluster { cluster_id } => {
                warn!(
                    "Cluster-based backup scheduling not yet implemented for cluster '{}'",
                    cluster_id
                );
                Err(anyhow::anyhow!(
                    "Cluster-based backup scheduling not yet implemented"
                ))
            }
            BackupTarget::Node { node_id } => {
                warn!(
                    "Node-based backup scheduling not yet implemented for node '{}'",
                    node_id
                );
                Err(anyhow::anyhow!(
                    "Node-based backup scheduling not yet implemented"
                ))
            }
        };

        let completed_at = Utc::now();
        let task_result = match result {
            Ok(backup_id) => {
                info!(
                    "Backup schedule '{}' completed successfully. Backup ID: {}",
                    schedule.id, backup_id
                );
                TaskResult {
                    schedule_id: schedule.id.clone(),
                    schedule_type: ScheduleType::Backup,
                    started_at,
                    completed_at,
                    success: true,
                    message: Some("Backup completed successfully".to_string()),
                    backup_id: Some(backup_id),
                }
            }
            Err(e) => {
                error!("Backup schedule '{}' failed: {}", schedule.id, e);
                TaskResult {
                    schedule_id: schedule.id.clone(),
                    schedule_type: ScheduleType::Backup,
                    started_at,
                    completed_at,
                    success: false,
                    message: Some(e.to_string()),
                    backup_id: None,
                }
            }
        };

        // Emit completion event
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(SchedulerEvent::TaskCompleted(task_result)).await;
        }
    }

    /// Execute a database backup.
    #[allow(clippy::too_many_arguments)]
    async fn execute_database_backup(
        &self,
        schedule_id: &str,
        host: &str,
        port: u16,
        database: &str,
        user: &str,
        backup_type: &BackupType,
        backup_dir: &Path,
        storage_profile: Option<&StorageProfile>,
        labels: &HashMap<String, String>,
    ) -> Result<String> {
        info!(
            "Executing {} backup for {}@{}:{}/{}",
            backup_type, user, host, port, database
        );

        // Build storage options
        let storage_opts = if let Some(profile) = storage_profile {
            postgres::cli::commands::StorageOptions {
                remote_storage: true,
                provider_type: Some(profile.provider.clone()),
                bucket: Some(profile.bucket.clone()),
                prefix: profile.prefix.clone(),
                region: profile.region.clone(),
                endpoint: profile.endpoint.clone(),
                access_key: resolve_secret(&profile.access_key),
                secret_key: resolve_secret(&profile.secret_key),
                multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
            }
        } else {
            postgres::cli::commands::StorageOptions {
                remote_storage: false,
                provider_type: None,
                bucket: None,
                prefix: None,
                region: None,
                endpoint: None,
                access_key: None,
                secret_key: None,
                multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
            }
        };

        // No SSH for now (can be extended later)
        let ssh_opts = postgres::cli::commands::SshOptions {
            host: None,
            user: None,
            port: None,
            password: None,
            key_path: None,
            local_port: None,
            remote_port: None,
        };

        // Add schedule ID to labels
        let mut all_labels = labels.clone();
        all_labels.insert("schedule_id".to_string(), schedule_id.to_string());

        // Execute the appropriate backup type
        match backup_type {
            BackupType::Snapshot => {
                let result = postgres::cli::commands::snapshot_backup(
                    host.to_string(),
                    port,
                    database.to_string(),
                    user.to_string(),
                    None, // password from env
                    None, // ssl_mode
                    backup_dir.to_path_buf(),
                    ssh_opts,
                    storage_opts,
                    all_labels,
                )
                .await
                .context("Snapshot backup failed")?;

                Ok(result.backup_id)
            }
            BackupType::Full => {
                postgres::cli::commands::full_backup(
                    host.to_string(),
                    port,
                    database.to_string(),
                    user.to_string(),
                    None,
                    None,
                    backup_dir.to_path_buf(),
                    ssh_opts,
                    storage_opts,
                )
                .await
                .context("Full backup failed")?;

                // Full backup doesn't return an ID in the same way
                Ok(format!("full_backup_{}", Utc::now().format("%Y%m%d%H%M%S")))
            }
            BackupType::Incremental => {
                postgres::cli::commands::incremental_backup(
                    host.to_string(),
                    port,
                    database.to_string(),
                    user.to_string(),
                    None,
                    None,
                    backup_dir.to_path_buf(),
                    ssh_opts,
                    storage_opts,
                )
                .await
                .context("Incremental backup failed")?;

                Ok(format!(
                    "incremental_backup_{}",
                    Utc::now().format("%Y%m%d%H%M%S")
                ))
            }
        }
    }

    /// Execute a retention schedule.
    async fn execute_retention_schedule(
        &mut self,
        schedule: &RetentionSchedule,
        config: &ScheduleConfig,
    ) {
        let started_at = Utc::now();
        info!("Executing retention schedule '{}'", schedule.id);

        // Record that we're running this schedule
        self.last_runs.insert(schedule.id.clone(), started_at);

        // Emit start event
        if let Some(tx) = &self.event_tx {
            let _ = tx
                .send(SchedulerEvent::TaskStarted {
                    schedule_id: schedule.id.clone(),
                    schedule_type: ScheduleType::Retention,
                    started_at,
                })
                .await;
        }

        if self.options.dry_run {
            info!(
                "[DRY-RUN] Would execute retention schedule '{}' (apply={})",
                schedule.id, schedule.apply
            );
            // Emit TaskCompleted event for dry-run to maintain event consistency
            if let Some(tx) = &self.event_tx {
                let _ = tx
                    .send(SchedulerEvent::TaskCompleted(TaskResult {
                        schedule_id: schedule.id.clone(),
                        schedule_type: ScheduleType::Retention,
                        started_at,
                        completed_at: Utc::now(),
                        success: true,
                        message: Some("Dry-run completed".to_string()),
                        backup_id: None,
                    }))
                    .await;
            }
            return;
        }

        // Get storage profile if specified
        let storage_profile = schedule
            .storage_profile
            .as_ref()
            .and_then(|name| config.get_storage_profile(name));

        // Build storage options
        let storage_opts = if let Some(profile) = storage_profile {
            postgres::cli::commands::StorageOptions {
                remote_storage: true,
                provider_type: Some(profile.provider.clone()),
                bucket: Some(profile.bucket.clone()),
                prefix: profile.prefix.clone(),
                region: profile.region.clone(),
                endpoint: profile.endpoint.clone(),
                access_key: resolve_secret(&profile.access_key),
                secret_key: resolve_secret(&profile.secret_key),
                multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
            }
        } else {
            postgres::cli::commands::StorageOptions {
                remote_storage: false,
                provider_type: None,
                bucket: None,
                prefix: None,
                region: None,
                endpoint: None,
                access_key: None,
                secret_key: None,
                multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
            }
        };

        // Execute retention
        let result = if schedule.apply {
            // Actually apply retention (with auto-yes for scheduled runs)
            postgres::cli::commands::purge(storage_opts, true, true).await
        } else {
            // Just run the plan (dry-run)
            postgres::cli::commands::purge_plan(storage_opts, "table".to_string()).await
        };

        let completed_at = Utc::now();
        let task_result = match result {
            Ok(_) => {
                info!(
                    "Retention schedule '{}' completed successfully",
                    schedule.id
                );
                TaskResult {
                    schedule_id: schedule.id.clone(),
                    schedule_type: ScheduleType::Retention,
                    started_at,
                    completed_at,
                    success: true,
                    message: Some("Retention completed successfully".to_string()),
                    backup_id: None,
                }
            }
            Err(e) => {
                error!("Retention schedule '{}' failed: {}", schedule.id, e);
                TaskResult {
                    schedule_id: schedule.id.clone(),
                    schedule_type: ScheduleType::Retention,
                    started_at,
                    completed_at,
                    success: false,
                    message: Some(e.to_string()),
                    backup_id: None,
                }
            }
        };

        // Emit completion event
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(SchedulerEvent::TaskCompleted(task_result)).await;
        }
    }

    /// Signal the scheduler to shut down.
    pub fn shutdown(&mut self) {
        self.shutdown = true;
    }

    /// Get the next scheduled runs.
    pub fn next_runs(&self) -> Vec<ScheduledRun> {
        if let Some(config) = self.get_schedule_config() {
            config.next_runs(Utc::now())
        } else {
            Vec::new()
        }
    }

    /// Get the next N scheduled runs.
    pub fn next_n_runs(&self, n: usize) -> Vec<ScheduledRun> {
        self.next_runs().into_iter().take(n).collect()
    }
}

/// Resolve a secret value that may be an environment variable reference.
///
/// Supports format: "env:VAR_NAME" to read from environment variable.
fn resolve_secret(value: &Option<String>) -> Option<String> {
    value.as_ref().and_then(|v| {
        if let Some(env_var) = v.strip_prefix("env:") {
            std::env::var(env_var).ok()
        } else {
            Some(v.clone())
        }
    })
}

/// Run the scheduler as a standalone task.
///
/// This function creates a scheduler and runs it until shutdown.
pub async fn run_scheduler(
    config: Arc<Mutex<WardenConfig>>,
    options: SchedulerOptions,
    event_tx: Option<mpsc::Sender<SchedulerEvent>>,
) -> Result<()> {
    let mut scheduler = Scheduler::new(config, options);
    if let Some(tx) = event_tx {
        scheduler = scheduler.with_event_channel(tx);
    }
    scheduler.run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::schedule::{BackupTarget, BackupType};

    fn create_test_config() -> WardenConfig {
        WardenConfig {
            c2_server: "http://localhost:8080".to_string(),
            c2_auth: common::config::C2AuthConfig {
                id: "test".to_string(),
                secret: "test".to_string(),
            },
            features: common::config::FeaturesConfig {
                overwatch: false,
                postgres_backup: true,
            },
            mqtt: None,
            schedules: Some(ScheduleConfig {
                backups: vec![BackupSchedule {
                    id: "test-backup".to_string(),
                    name: Some("Test Backup".to_string()),
                    cron: "0 2 * * *".to_string(),
                    target: BackupTarget::Database {
                        host: "localhost".to_string(),
                        port: Some(5432),
                        database: "testdb".to_string(),
                        user: Some("postgres".to_string()),
                    },
                    backup_type: BackupType::Snapshot,
                    storage_profile: None,
                    enabled: true,
                    labels: HashMap::new(),
                    backup_dir: None,
                    encryption: None,
                }],
                retention: vec![],
                storage_profiles: vec![],
                default_backup_dir: Some("./backups".to_string()),
            }),
            integration: common::config::IntegrationConfig::default(),
            notifications: common::notifications::NotificationConfig::default(),
        }
    }

    #[test]
    fn test_scheduler_creation() {
        let config = Arc::new(Mutex::new(create_test_config()));
        let options = SchedulerOptions::default();
        let scheduler = Scheduler::new(config, options);
        assert!(!scheduler.shutdown);
    }

    #[test]
    fn test_next_runs() {
        let config = Arc::new(Mutex::new(create_test_config()));
        let options = SchedulerOptions::default();
        let scheduler = Scheduler::new(config, options);
        let runs = scheduler.next_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].schedule_id, "test-backup");
    }

    #[test]
    fn test_resolve_secret_plain() {
        let value = Some("plain_value".to_string());
        assert_eq!(resolve_secret(&value), Some("plain_value".to_string()));
    }

    #[test]
    fn test_resolve_secret_env() {
        // SAFETY: This test runs in isolation and the env var is cleaned up after
        unsafe {
            std::env::set_var("TEST_SECRET_VAR", "secret_from_env");
        }
        let value = Some("env:TEST_SECRET_VAR".to_string());
        assert_eq!(resolve_secret(&value), Some("secret_from_env".to_string()));
        // SAFETY: Cleanup the test env var
        unsafe {
            std::env::remove_var("TEST_SECRET_VAR");
        }
    }

    #[test]
    fn test_resolve_secret_missing_env() {
        let value = Some("env:NONEXISTENT_VAR_12345".to_string());
        assert_eq!(resolve_secret(&value), None);
    }
}
