//! Schedule CLI command implementations.
//!
//! This module provides CLI commands for viewing and managing backup/retention schedules.

use anyhow::{anyhow, Result};
use chrono::Utc;
use common::config::load_config;
use common::schedule::{
    BackupSchedule, BackupTarget, ParsedSchedule, RetentionSchedule, ScheduleConfig, ScheduleType,
    ScheduledRun,
};
use log::info;
use serde::Serialize;

/// Output format for schedule commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
}

impl OutputFormat {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => OutputFormat::Json,
            _ => OutputFormat::Table,
        }
    }
}

/// Information about a backup schedule for display.
#[derive(Debug, Clone, Serialize)]
pub struct BackupScheduleInfo {
    pub id: String,
    pub name: Option<String>,
    pub cron: String,
    pub backup_type: String,
    pub target: String,
    pub enabled: bool,
    pub storage_profile: Option<String>,
}

impl From<&BackupSchedule> for BackupScheduleInfo {
    fn from(s: &BackupSchedule) -> Self {
        let target = match &s.target {
            BackupTarget::Database {
                host,
                port,
                database,
                ..
            } => format!("{}:{}/{}", host, port.unwrap_or(5432), database),
            BackupTarget::Cluster { cluster_id } => format!("cluster:{}", cluster_id),
            BackupTarget::Node { node_id } => format!("node:{}", node_id),
        };

        BackupScheduleInfo {
            id: s.id.clone(),
            name: s.name.clone(),
            cron: s.cron.clone(),
            backup_type: s.backup_type.to_string(),
            target,
            enabled: s.enabled,
            storage_profile: s.storage_profile.clone(),
        }
    }
}

/// Information about a retention schedule for display.
#[derive(Debug, Clone, Serialize)]
pub struct RetentionScheduleInfo {
    pub id: String,
    pub name: Option<String>,
    pub cron: String,
    pub apply: bool,
    pub enabled: bool,
    pub storage_profile: Option<String>,
    pub policy_file: Option<String>,
}

impl From<&RetentionSchedule> for RetentionScheduleInfo {
    fn from(s: &RetentionSchedule) -> Self {
        RetentionScheduleInfo {
            id: s.id.clone(),
            name: s.name.clone(),
            cron: s.cron.clone(),
            apply: s.apply,
            enabled: s.enabled,
            storage_profile: s.storage_profile.clone(),
            policy_file: s.policy_file.clone(),
        }
    }
}

/// Combined schedule list result.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleListResult {
    pub backups: Vec<BackupScheduleInfo>,
    pub retention: Vec<RetentionScheduleInfo>,
}

/// List all configured schedules.
pub async fn schedule_list(
    format: String,
    enabled_only: bool,
    schedule_type: Option<String>,
) -> Result<()> {
    let config = load_config().map_err(|e| anyhow!("Failed to load configuration: {}", e))?;

    let schedule_config = config
        .schedules
        .ok_or_else(|| anyhow!("No schedules configured in warden configuration"))?;

    let output_format = OutputFormat::from_str(&format);

    // Filter schedules
    let type_filter = schedule_type.as_ref().map(|t| t.to_lowercase());

    let backups: Vec<BackupScheduleInfo> = schedule_config
        .backups
        .iter()
        .filter(|s| !enabled_only || s.enabled)
        .filter(|_| {
            type_filter.is_none()
                || type_filter.as_ref().map(|t| t == "backup").unwrap_or(false)
        })
        .map(BackupScheduleInfo::from)
        .collect();

    let retention: Vec<RetentionScheduleInfo> = schedule_config
        .retention
        .iter()
        .filter(|s| !enabled_only || s.enabled)
        .filter(|_| {
            type_filter.is_none()
                || type_filter.as_ref().map(|t| t == "retention").unwrap_or(false)
        })
        .map(RetentionScheduleInfo::from)
        .collect();

    let result = ScheduleListResult { backups, retention };

    match output_format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&result)
                    .map_err(|e| anyhow!("Failed to serialize: {}", e))?
            );
        }
        OutputFormat::Table => {
            print_schedule_list_table(&result);
        }
    }

    Ok(())
}

fn print_schedule_list_table(result: &ScheduleListResult) {
    if result.backups.is_empty() && result.retention.is_empty() {
        println!("No schedules configured.");
        return;
    }

    if !result.backups.is_empty() {
        println!("\n=== Backup Schedules ({}) ===\n", result.backups.len());
        println!(
            "{:<20} {:<30} {:<20} {:<12} {:<10} {:<8}",
            "ID", "CRON", "TARGET", "TYPE", "STORAGE", "ENABLED"
        );
        println!("{}", "-".repeat(100));

        for schedule in &result.backups {
            let enabled_str = if schedule.enabled { "✓" } else { "✗" };
            let storage = schedule
                .storage_profile
                .as_deref()
                .unwrap_or("-");
            let target = if schedule.target.len() > 28 {
                format!("{}...", &schedule.target[..25])
            } else {
                schedule.target.clone()
            };

            println!(
                "{:<20} {:<30} {:<20} {:<12} {:<10} {:<8}",
                schedule.id, schedule.cron, target, schedule.backup_type, storage, enabled_str
            );
        }
    }

    if !result.retention.is_empty() {
        println!(
            "\n=== Retention Schedules ({}) ===\n",
            result.retention.len()
        );
        println!(
            "{:<20} {:<30} {:<10} {:<10} {:<8}",
            "ID", "CRON", "STORAGE", "APPLY", "ENABLED"
        );
        println!("{}", "-".repeat(78));

        for schedule in &result.retention {
            let enabled_str = if schedule.enabled { "✓" } else { "✗" };
            let apply_str = if schedule.apply { "yes" } else { "dry-run" };
            let storage = schedule
                .storage_profile
                .as_deref()
                .unwrap_or("-");

            println!(
                "{:<20} {:<30} {:<10} {:<10} {:<8}",
                schedule.id, schedule.cron, storage, apply_str, enabled_str
            );
        }
    }

    println!();
}

/// Show the next scheduled runs.
pub async fn schedule_next_runs(count: usize, format: String, enabled_only: bool) -> Result<()> {
    let config = load_config().map_err(|e| anyhow!("Failed to load configuration: {}", e))?;

    let schedule_config = config
        .schedules
        .ok_or_else(|| anyhow!("No schedules configured in warden configuration"))?;

    let output_format = OutputFormat::from_str(&format);

    // Get next runs for all schedules
    let now = Utc::now();
    let mut all_runs: Vec<ScheduledRun> = Vec::new();

    for schedule in &schedule_config.backups {
        if enabled_only && !schedule.enabled {
            continue;
        }

        if let Ok(parsed) = ParsedSchedule::new(schedule.id.clone(), &schedule.cron) {
            let next_times = parsed.next_n_after(now, count);
            for next_time in next_times {
                all_runs.push(ScheduledRun {
                    schedule_id: schedule.id.clone(),
                    schedule_name: schedule.name.clone(),
                    schedule_type: ScheduleType::Backup,
                    next_run: next_time,
                    enabled: schedule.enabled,
                });
            }
        }
    }

    for schedule in &schedule_config.retention {
        if enabled_only && !schedule.enabled {
            continue;
        }

        if let Ok(parsed) = ParsedSchedule::new(schedule.id.clone(), &schedule.cron) {
            let next_times = parsed.next_n_after(now, count);
            for next_time in next_times {
                all_runs.push(ScheduledRun {
                    schedule_id: schedule.id.clone(),
                    schedule_name: schedule.name.clone(),
                    schedule_type: ScheduleType::Retention,
                    next_run: next_time,
                    enabled: schedule.enabled,
                });
            }
        }
    }

    // Sort by next run time
    all_runs.sort_by_key(|r| r.next_run);

    // Limit total results
    let all_runs: Vec<_> = all_runs.into_iter().take(count * 2).collect();

    match output_format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&all_runs)
                    .map_err(|e| anyhow!("Failed to serialize: {}", e))?
            );
        }
        OutputFormat::Table => {
            print_next_runs_table(&all_runs, now);
        }
    }

    Ok(())
}

fn print_next_runs_table(runs: &[ScheduledRun], now: chrono::DateTime<Utc>) {
    if runs.is_empty() {
        println!("No upcoming scheduled runs.");
        return;
    }

    println!("\n=== Upcoming Scheduled Runs ===\n");
    println!(
        "{:<20} {:<10} {:<25} {:<15}",
        "SCHEDULE ID", "TYPE", "NEXT RUN (UTC)", "IN"
    );
    println!("{}", "-".repeat(70));

    for run in runs {
        let duration = run.next_run - now;
        let in_str = format_duration(duration);

        println!(
            "{:<20} {:<10} {:<25} {:<15}",
            run.schedule_id,
            run.schedule_type.to_string(),
            run.next_run.format("%Y-%m-%d %H:%M:%S"),
            in_str
        );
    }

    println!();
}

fn format_duration(duration: chrono::Duration) -> String {
    let total_secs = duration.num_seconds();
    if total_secs < 0 {
        return "overdue".to_string();
    }

    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

/// Validate schedule configuration.
pub async fn schedule_validate() -> Result<()> {
    let config = load_config().map_err(|e| anyhow!("Failed to load configuration: {}", e))?;

    let schedule_config = match config.schedules {
        Some(c) => c,
        None => {
            println!("No schedules configured in warden configuration.");
            return Ok(());
        }
    };

    match schedule_config.validate() {
        Ok(()) => {
            println!("✓ Schedule configuration is valid.");
            println!("  - {} backup schedule(s)", schedule_config.backups.len());
            println!(
                "  - {} retention schedule(s)",
                schedule_config.retention.len()
            );
            println!(
                "  - {} storage profile(s)",
                schedule_config.storage_profiles.len()
            );
            Ok(())
        }
        Err(e) => {
            println!("✗ Schedule configuration is invalid:");
            println!("  Error: {}", e);
            Err(anyhow!("Schedule validation failed: {}", e))
        }
    }
}

/// Run a specific schedule immediately.
pub async fn schedule_run(id: String, dry_run: bool) -> Result<()> {
    let config = load_config().map_err(|e| anyhow!("Failed to load configuration: {}", e))?;

    let schedule_config = config
        .schedules
        .ok_or_else(|| anyhow!("No schedules configured in warden configuration"))?;

    // Find the schedule
    let backup_schedule = schedule_config.backups.iter().find(|s| s.id == id);
    let retention_schedule = schedule_config.retention.iter().find(|s| s.id == id);

    match (backup_schedule, retention_schedule) {
        (Some(schedule), None) => {
            run_backup_schedule(schedule, &schedule_config, dry_run).await
        }
        (None, Some(schedule)) => {
            run_retention_schedule(schedule, &schedule_config, dry_run).await
        }
        (None, None) => Err(anyhow!("Schedule '{}' not found", id)),
        (Some(_), Some(_)) => Err(anyhow!(
            "Ambiguous schedule ID '{}' - found in both backup and retention",
            id
        )),
    }
}

async fn run_backup_schedule(
    schedule: &BackupSchedule,
    config: &ScheduleConfig,
    dry_run: bool,
) -> Result<()> {
    info!(
        "Running backup schedule '{}' (type: {})",
        schedule.id, schedule.backup_type
    );

    if dry_run {
        println!("[DRY-RUN] Would execute backup schedule '{}'", schedule.id);
        println!("  Type: {}", schedule.backup_type);
        println!("  Target: {:?}", schedule.target);
        if let Some(ref profile) = schedule.storage_profile {
            println!("  Storage profile: {}", profile);
        }
        return Ok(());
    }

    // Get storage profile
    let storage_profile = schedule
        .storage_profile
        .as_ref()
        .and_then(|name| config.get_storage_profile(name));

    // Build storage options
    let storage_opts = if let Some(profile) = storage_profile {
        super::StorageOptions {
            remote_storage: true,
            provider_type: Some(profile.provider.clone()),
            bucket: Some(profile.bucket.clone()),
            prefix: profile.prefix.clone(),
            region: profile.region.clone(),
            endpoint: profile.endpoint.clone(),
            access_key: resolve_secret(&profile.access_key),
            secret_key: resolve_secret(&profile.secret_key),
            multi_tenant: super::MultiTenantOptions::default(),
        }
    } else {
        super::StorageOptions::default()
    };

    let ssh_opts = super::SshOptions::default();

    // Determine backup directory
    let backup_dir = schedule
        .backup_dir
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(|| config.default_backup_dir.as_ref().map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("./backups"));

    // Execute based on target
    match &schedule.target {
        BackupTarget::Database {
            host,
            port,
            database,
            user,
        } => {
            let mut labels = schedule.labels.clone();
            labels.insert("schedule_id".to_string(), schedule.id.clone());
            labels.insert("manual_run".to_string(), "true".to_string());

            let result = super::snapshot_backup(
                host.clone(),
                port.unwrap_or(5432),
                database.clone(),
                user.clone().unwrap_or_else(|| "postgres".to_string()),
                None,
                None,
                backup_dir,
                ssh_opts,
                storage_opts,
                labels,
            )
            .await?;

            println!("Backup completed successfully!");
            println!("  Backup ID: {}", result.backup_id);
            println!("  Local path: {}", result.local_path.display());
            if let Some(remote) = result.remote_path {
                println!("  Remote path: {}", remote);
            }

            Ok(())
        }
        BackupTarget::Cluster { cluster_id } => {
            Err(anyhow!(
                "Cluster-based backup scheduling not yet implemented for cluster '{}'",
                cluster_id
            ))
        }
        BackupTarget::Node { node_id } => {
            Err(anyhow!(
                "Node-based backup scheduling not yet implemented for node '{}'",
                node_id
            ))
        }
    }
}

async fn run_retention_schedule(
    schedule: &RetentionSchedule,
    config: &ScheduleConfig,
    dry_run: bool,
) -> Result<()> {
    info!("Running retention schedule '{}'", schedule.id);

    if dry_run {
        println!(
            "[DRY-RUN] Would execute retention schedule '{}'",
            schedule.id
        );
        println!("  Apply: {}", schedule.apply);
        if let Some(ref profile) = schedule.storage_profile {
            println!("  Storage profile: {}", profile);
        }
        return Ok(());
    }

    // Get storage profile
    let storage_profile = schedule
        .storage_profile
        .as_ref()
        .and_then(|name| config.get_storage_profile(name));

    // Build storage options
    let storage_opts = if let Some(profile) = storage_profile {
        super::StorageOptions {
            remote_storage: true,
            provider_type: Some(profile.provider.clone()),
            bucket: Some(profile.bucket.clone()),
            prefix: profile.prefix.clone(),
            region: profile.region.clone(),
            endpoint: profile.endpoint.clone(),
            access_key: resolve_secret(&profile.access_key),
            secret_key: resolve_secret(&profile.secret_key),
            multi_tenant: super::MultiTenantOptions::default(),
        }
    } else {
        super::StorageOptions::default()
    };

    if schedule.apply {
        super::purge(storage_opts, true, true).await?;
        println!("Retention policy applied successfully!");
    } else {
        super::purge_plan(storage_opts, "table".to_string()).await?;
        println!("Retention plan computed (dry-run mode).");
    }

    Ok(())
}

/// Resolve a secret value that may be an environment variable reference.
fn resolve_secret(value: &Option<String>) -> Option<String> {
    value.as_ref().and_then(|v| {
        if let Some(env_var) = v.strip_prefix("env:") {
            std::env::var(env_var).ok()
        } else {
            Some(v.clone())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("json"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("JSON"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("table"), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str("unknown"), OutputFormat::Table);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(chrono::Duration::minutes(30)), "30m");
        assert_eq!(format_duration(chrono::Duration::hours(2)), "2h 0m");
        assert_eq!(
            format_duration(chrono::Duration::hours(25)),
            "1d 1h"
        );
    }
}
