//! Status command handlers for observability.

use anyhow::{anyhow, Result};
use log::info;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::status::collector::StorageConfig;
use crate::status::{
    format_duration, format_size, HealthStatus, Metrics, OverallStatus, StatusCollector,
    StatusCollectorConfig, StatusThresholds,
};

/// Storage options for status commands.
pub struct StatusStorageOptions {
    pub remote_storage: bool,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    /// Multi-tenant organization options
    pub multi_tenant: super::MultiTenantOptions,
}

/// Create a storage config from options.
fn create_storage_config(opts: &StatusStorageOptions) -> Result<Option<StorageConfig>> {
    if !opts.remote_storage {
        return Ok(None);
    }

    let bucket = opts
        .bucket
        .as_ref()
        .ok_or_else(|| anyhow!("--storage-bucket is required when --remote-storage is set"))?;

    Ok(Some(StorageConfig {
        bucket: bucket.clone(),
        prefix: opts.prefix.clone(),
        region: opts.region.clone(),
        endpoint: opts.endpoint.clone(),
        access_key: opts.access_key.clone(),
        secret_key: opts.secret_key.clone(),
    }))
}

/// Execute the status command.
#[allow(clippy::too_many_arguments)]
pub async fn execute_status(
    backup_dir: PathBuf,
    wal_archive_dir: Option<PathBuf>,
    retention_policy: Option<PathBuf>,
    database: Option<String>,
    host: Option<String>,
    storage_opts: StatusStorageOptions,
    format: String,
    backup_warning_age_hours: u32,
    backup_critical_age_hours: u32,
) -> Result<()> {
    info!("[status] Collecting data protection status...");

    let storage_config = create_storage_config(&storage_opts)?;

    let config = StatusCollectorConfig {
        backup_dir,
        wal_archive_dir,
        storage_config,
        retention_policy_path: retention_policy,
        database: database.clone(),
        host: host.clone(),
        thresholds: StatusThresholds {
            backup_warning_age_hours,
            backup_critical_age_hours,
            ..Default::default()
        },
    };

    let collector = StatusCollector::new(config);
    let status = collector.collect_status().await?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&status)
                .map_err(|e| anyhow!("Failed to serialize status: {}", e))?
        );
    } else {
        print_status_table(&status, database.as_deref(), host.as_deref());
    }

    // Return appropriate exit code based on health
    match status.health {
        HealthStatus::Critical => std::process::exit(2),
        HealthStatus::Warning => std::process::exit(1),
        _ => Ok(()),
    }
}

/// Print status in table format.
fn print_status_table(status: &OverallStatus, database: Option<&str>, host: Option<&str>) {
    let reset = "\x1b[0m";

    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                    WARDEN DATA PROTECTION STATUS                 ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    // Context
    if database.is_some() || host.is_some() {
        print!("  Context: ");
        if let Some(h) = host {
            print!("{}:{}", h, database.unwrap_or("*"));
        } else if let Some(db) = database {
            print!("{}", db);
        }
        println!();
        println!();
    }

    // Overall health
    let health_color = status.health.color_code();
    let health_emoji = status.health.emoji();
    println!(
        "  Overall Health: {}{} {}{}",
        health_color,
        health_emoji,
        status.health.to_string().to_uppercase(),
        reset
    );
    println!(
        "  Collected At:   {}",
        status.collected_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!();

    // Backup status
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ BACKUP STATUS                                                   │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let backup = &status.backup;
    let backup_color = backup.health.color_code();
    println!(
        "│  Health:           {}{} {}{}",
        backup_color,
        backup.health.emoji(),
        backup.health.to_string().to_uppercase(),
        reset
    );
    println!("│  Total Backups:    {}", backup.total_backups);
    println!("│  Successful:       {}", backup.successful_backups);
    println!("│  Failed:           {}", backup.failed_backups);

    if let Some(ref last) = backup.last_successful {
        println!(
            "│  Last Successful:  {} ({})",
            last.start_time.format("%Y-%m-%d %H:%M:%S UTC"),
            last.backup_type
        );
        if let Some(age) = backup.last_backup_age {
            println!("│  Backup Age:       {}", format_duration(age));
        }
        println!("│  Backup Size:      {}", format_size(last.size_bytes));
    } else {
        println!("│  Last Successful:  None");
    }

    if let Some(interval) = backup.average_interval {
        println!("│  Avg Interval:     {}", format_duration(interval));
    }
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();

    // PITR status
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ PITR STATUS                                                     │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let pitr = &status.pitr;
    let pitr_color = pitr.health.color_code();
    println!(
        "│  Health:           {}{} {}{}",
        pitr_color,
        pitr.health.emoji(),
        pitr.health.to_string().to_uppercase(),
        reset
    );
    println!(
        "│  Available:        {}",
        if pitr.available { "Yes" } else { "No" }
    );

    if let Some(earliest) = pitr.earliest_recovery_point {
        println!(
            "│  Earliest Point:   {}",
            earliest.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }
    if let Some(latest) = pitr.latest_recovery_point {
        println!(
            "│  Latest Point:     {}",
            latest.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }
    if let Some(window) = pitr.recovery_window {
        println!("│  Recovery Window:  {}", format_duration(window));
    }
    println!("│  Base Backups:     {}", pitr.base_backup_count);
    println!("│  WAL Segments:     {}", pitr.wal_segment_count);
    println!("│  WAL Size:         {}", format_size(pitr.wal_size_bytes));

    if !pitr.wal_gaps.is_empty() {
        println!(
            "│  WAL Gaps:         {} (recovery may fail)",
            pitr.wal_gaps.len()
        );
    }
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();

    // Retention status
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ RETENTION STATUS                                                │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    let retention = &status.retention;
    let retention_color = retention.health.color_code();
    println!(
        "│  Health:           {}{} {}{}",
        retention_color,
        retention.health.emoji(),
        retention.health.to_string().to_uppercase(),
        reset
    );
    println!(
        "│  Policy Configured: {}",
        if retention.policy_configured {
            "Yes"
        } else {
            "No"
        }
    );

    if let Some(ref name) = retention.policy_name {
        println!("│  Policy Version:   {}", name);
    }
    if let Some(hours) = retention.pitr_window_hours {
        println!("│  PITR Window:      {} hours", hours);
    }
    if let Some(min) = retention.min_backups_to_keep {
        println!("│  Min Backups:      {}", min);
    }
    if retention.pending_deletions > 0 {
        println!("│  Pending Deletes:  {}", retention.pending_deletions);
        println!(
            "│  Space to Free:    {}",
            format_size(retention.pending_space_freed)
        );
    }
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();

    // Storage status
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ STORAGE STATUS                                                  │");
    println!("├─────────────────────────────────────────────────────────────────┤");

    if let Some(ref local) = status.storage.local {
        println!("│  Local Storage:");
        println!("│    Location:       {}", local.location);
        println!("│    Total Used:     {}", format_size(local.used_bytes));
        println!(
            "│    Backups:        {} ({})",
            local.backup_count,
            format_size(local.backup_size_bytes)
        );
        println!(
            "│    WAL:            {} ({})",
            local.wal_count,
            format_size(local.wal_size_bytes)
        );
    }

    if let Some(ref remote) = status.storage.remote {
        println!("│  Remote Storage:");
        println!("│    Location:       {}", remote.location);
        println!("│    Total Used:     {}", format_size(remote.used_bytes));
        println!(
            "│    Backups:        {} ({})",
            remote.backup_count,
            format_size(remote.backup_size_bytes)
        );
        println!(
            "│    WAL:            {} ({})",
            remote.wal_count,
            format_size(remote.wal_size_bytes)
        );
    }

    if status.storage.local.is_none() && status.storage.remote.is_none() {
        println!("│  No storage information available");
    }
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();

    // Issues
    if !status.issues.is_empty() {
        println!("┌─────────────────────────────────────────────────────────────────┐");
        println!("│ ISSUES                                                          │");
        println!("├─────────────────────────────────────────────────────────────────┤");
        for issue in &status.issues {
            let color = issue.severity.color_code();
            println!(
                "│  {}{}{} [{}] {}",
                color,
                issue.severity.emoji(),
                reset,
                issue.category,
                issue.message
            );
        }
        println!("└─────────────────────────────────────────────────────────────────┘");
        println!();
    }
}

/// Execute the backup-status command.
pub async fn execute_backup_status(
    backup_dir: PathBuf,
    database: Option<String>,
    storage_opts: StatusStorageOptions,
    format: String,
) -> Result<()> {
    info!("[backup-status] Collecting backup status...");

    let storage_config = create_storage_config(&storage_opts)?;

    let config = StatusCollectorConfig {
        backup_dir,
        wal_archive_dir: None,
        storage_config,
        retention_policy_path: None,
        database: database.clone(),
        host: None,
        thresholds: StatusThresholds::default(),
    };

    let collector = StatusCollector::new(config);
    let status = collector.collect_backup_status().await?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&status)
                .map_err(|e| anyhow!("Failed to serialize status: {}", e))?
        );
    } else {
        print_backup_status_table(&status);
    }

    Ok(())
}

/// Print backup status in table format.
fn print_backup_status_table(status: &crate::status::BackupStatus) {
    let reset = "\x1b[0m";
    let health_color = status.health.color_code();

    println!();
    println!("=== Backup Status ===");
    println!();
    println!(
        "Health:         {}{} {}{}",
        health_color,
        status.health.emoji(),
        status.health.to_string().to_uppercase(),
        reset
    );
    println!("Total Backups:  {}", status.total_backups);
    println!("Successful:     {}", status.successful_backups);
    println!("Failed:         {}", status.failed_backups);
    println!();

    if let Some(ref last) = status.last_successful {
        println!("=== Last Successful Backup ===");
        println!();
        println!("ID:             {}", last.id);
        println!("Type:           {}", last.backup_type);
        println!(
            "Time:           {}",
            last.start_time.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!("Size:           {}", format_size(last.size_bytes));
        if let Some(ref loc) = last.location {
            println!("Location:       {}", loc);
        }
        println!();
    }

    if let Some(age) = status.last_backup_age {
        println!("Backup Age:     {}", format_duration(age));
    }
    if let Some(interval) = status.average_interval {
        println!("Avg Interval:   {}", format_duration(interval));
    }

    if !status.issues.is_empty() {
        println!();
        println!("=== Issues ===");
        for issue in &status.issues {
            println!("  - {}", issue);
        }
    }
}

/// Execute the pitr-status command.
pub async fn execute_pitr_status(
    backup_dir: PathBuf,
    wal_archive_dir: Option<PathBuf>,
    database: Option<String>,
    storage_opts: StatusStorageOptions,
    format: String,
) -> Result<()> {
    info!("[pitr-status] Collecting PITR status...");

    let storage_config = create_storage_config(&storage_opts)?;

    let config = StatusCollectorConfig {
        backup_dir,
        wal_archive_dir,
        storage_config,
        retention_policy_path: None,
        database,
        host: None,
        thresholds: StatusThresholds::default(),
    };

    let collector = StatusCollector::new(config);
    let status = collector.collect_pitr_status().await?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&status)
                .map_err(|e| anyhow!("Failed to serialize status: {}", e))?
        );
    } else {
        print_pitr_status_table(&status);
    }

    Ok(())
}

/// Print PITR status in table format.
fn print_pitr_status_table(status: &crate::status::PitrStatus) {
    let reset = "\x1b[0m";
    let health_color = status.health.color_code();

    println!();
    println!("=== PITR Status ===");
    println!();
    println!(
        "Health:           {}{} {}{}",
        health_color,
        status.health.emoji(),
        status.health.to_string().to_uppercase(),
        reset
    );
    println!(
        "Available:        {}",
        if status.available { "Yes" } else { "No" }
    );
    println!();

    println!("=== Recovery Window ===");
    println!();
    if let Some(earliest) = status.earliest_recovery_point {
        println!(
            "Earliest Point:   {}",
            earliest.format("%Y-%m-%d %H:%M:%S UTC")
        );
    } else {
        println!("Earliest Point:   N/A");
    }
    if let Some(latest) = status.latest_recovery_point {
        println!(
            "Latest Point:     {}",
            latest.format("%Y-%m-%d %H:%M:%S UTC")
        );
    } else {
        println!("Latest Point:     N/A");
    }
    if let Some(window) = status.recovery_window {
        println!("Window Size:      {}", format_duration(window));
    }
    println!();

    println!("=== WAL Coverage ===");
    println!();
    println!("Base Backups:     {}", status.base_backup_count);
    println!("WAL Segments:     {}", status.wal_segment_count);
    println!("WAL Size:         {}", format_size(status.wal_size_bytes));

    if !status.wal_gaps.is_empty() {
        println!();
        println!("=== WAL Gaps (Recovery may fail in these ranges) ===");
        for gap in &status.wal_gaps {
            println!("  {} -> {}", gap.start, gap.end);
        }
    }

    if !status.issues.is_empty() {
        println!();
        println!("=== Issues ===");
        for issue in &status.issues {
            println!("  - {}", issue);
        }
    }
}

/// Execute the metrics command.
#[allow(clippy::too_many_arguments)]
pub async fn execute_metrics(
    backup_dir: PathBuf,
    wal_archive_dir: Option<PathBuf>,
    database: Option<String>,
    host: Option<String>,
    storage_opts: StatusStorageOptions,
    output: Option<PathBuf>,
    format: String,
) -> Result<()> {
    info!("[metrics] Collecting metrics...");

    let storage_config = create_storage_config(&storage_opts)?;

    let config = StatusCollectorConfig {
        backup_dir,
        wal_archive_dir,
        storage_config,
        retention_policy_path: None,
        database: database.clone(),
        host: host.clone(),
        thresholds: StatusThresholds::default(),
    };

    let collector = StatusCollector::new(config);
    let gauges = collector.collect_metrics().await?;

    // Create metrics with labels
    let mut labels = HashMap::new();
    if let Some(db) = database {
        labels.insert("database".to_string(), db);
    }
    if let Some(h) = host {
        labels.insert("host".to_string(), h);
    }

    let metrics = Metrics::with_labels(labels);
    metrics.update_gauges(gauges);

    let output_str = if format == "json" {
        metrics
            .export_json()
            .map_err(|e| anyhow!("Failed to export metrics: {}", e))?
    } else {
        metrics.export_prometheus()
    };

    if let Some(path) = output {
        std::fs::write(&path, &output_str)
            .map_err(|e| anyhow!("Failed to write metrics to {}: {}", path.display(), e))?;
        info!("[metrics] Metrics written to {}", path.display());
    } else {
        println!("{}", output_str);
    }

    Ok(())
}
