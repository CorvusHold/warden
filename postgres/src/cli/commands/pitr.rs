//! PITR CLI command implementations.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use dialoguer::Confirm;
use log::{error, info};
use serde::Serialize;
use std::path::PathBuf;

use storage::{PostgresBackupStorage, StorageProviderType};

use crate::pitr::{PitrExecutor, PitrPlanner, RecoveryTarget};

/// Options for PITR storage configuration
#[derive(Clone, Debug)]
pub struct PitrStorageOptions {
    pub remote_storage: bool,
    pub provider_type: StorageProviderType,
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub wal_prefix: String,
}

impl Default for PitrStorageOptions {
    fn default() -> Self {
        Self {
            remote_storage: false,
            provider_type: StorageProviderType::S3,
            bucket: None,
            prefix: None,
            region: None,
            endpoint: None,
            access_key: None,
            secret_key: None,
            wal_prefix: String::new(),
        }
    }
}

/// Result of PITR plan command
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PitrPlanResult {
    pub plan_id: String,
    #[serde(serialize_with = "serialize_datetime")]
    pub target_time: DateTime<Utc>,
    pub base_backup_id: String,
    #[serde(serialize_with = "serialize_datetime")]
    pub base_backup_time: DateTime<Utc>,
    pub wal_segment_count: usize,
    pub estimated_download_bytes: u64,
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

fn serialize_datetime<S>(dt: &DateTime<Utc>, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&dt.to_rfc3339())
}

/// Compute a PITR recovery plan
#[allow(clippy::too_many_arguments)]
pub async fn pitr_plan(
    target_time: String,
    backup_dir: PathBuf,
    wal_archive_dir: Option<PathBuf>,
    storage_opts: PitrStorageOptions,
    format: String,
) -> Result<PitrPlanResult> {
    info!(
        "[pitr-plan] Computing recovery plan for target time: {}",
        target_time
    );

    // Parse target time
    let target = RecoveryTarget::parse(&target_time)
        .map_err(|e| anyhow!("Invalid target time '{}': {}", target_time, e))?;

    let target_dt = target
        .as_time()
        .ok_or_else(|| anyhow!("Target must be a valid RFC3339 timestamp"))?;

    // Create planner
    let mut planner = PitrPlanner::new(backup_dir.clone());

    // Add WAL archive directory if specified
    if let Some(wal_dir) = wal_archive_dir {
        planner = planner.with_wal_archive_dir(wal_dir);
    }

    // Add WAL prefix
    planner = planner.with_wal_prefix(storage_opts.wal_prefix.clone());

    // Add remote storage if configured
    if storage_opts.remote_storage {
        let storage = create_storage(&storage_opts).await?;
        planner = planner.with_storage(storage);
    }

    // Compute the plan
    let plan = planner
        .plan_recovery(target)
        .await
        .map_err(|e| anyhow!("Failed to compute recovery plan: {}", e))?;

    let result = PitrPlanResult {
        plan_id: plan.id.to_string(),
        target_time: target_dt,
        base_backup_id: plan.base_backup.id.to_string(),
        base_backup_time: plan.base_backup.start_time,
        wal_segment_count: plan.wal_segments.len(),
        estimated_download_bytes: plan.estimated_download_bytes,
        is_valid: plan.validation.is_valid,
        errors: plan.validation.errors.clone(),
        warnings: plan.validation.warnings.clone(),
    };

    // Output the plan
    match format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&result)?;
            println!("{}", json);
        }
        _ => {
            println!("\n=== PITR Recovery Plan ===\n");
            println!("Plan ID:           {}", result.plan_id);
            println!("Target Time:       {}", result.target_time);
            println!();
            println!("Base Backup:");
            println!("  ID:              {}", result.base_backup_id);
            println!("  Start Time:      {}", result.base_backup_time);
            println!("  Server Version:  {}", plan.base_backup.server_version);
            println!(
                "  Size:            {} bytes ({:.2} MB)",
                plan.base_backup.size_bytes,
                plan.base_backup.size_bytes as f64 / 1024.0 / 1024.0
            );
            println!("  Remote:          {}", plan.base_backup.is_remote);
            println!();
            println!("WAL Segments:      {}", result.wal_segment_count);
            println!(
                "Est. Download:     {} bytes ({:.2} MB)",
                result.estimated_download_bytes,
                result.estimated_download_bytes as f64 / 1024.0 / 1024.0
            );
            println!();
            println!("Recovery Window:");
            println!("  Earliest:        {}", plan.recovery_window.earliest);
            if let Some(latest) = plan.recovery_window.latest {
                println!("  Latest:          {}", latest);
            } else {
                println!("  Latest:          (unknown - no WAL timestamps)");
            }
            println!();
            println!(
                "Validation:        {}",
                if result.is_valid {
                    "✓ VALID"
                } else {
                    "✗ INVALID"
                }
            );

            if !result.errors.is_empty() {
                println!("\nErrors:");
                for err in &result.errors {
                    println!("  ✗ {}", err);
                }
            }

            if !result.warnings.is_empty() {
                println!("\nWarnings:");
                for warn in &result.warnings {
                    println!("  ⚠ {}", warn);
                }
            }
            println!();
        }
    }

    if !result.is_valid {
        return Err(anyhow!(
            "Recovery plan is invalid: {}",
            result.errors.join("; ")
        ));
    }

    info!("[pitr-plan] Recovery plan computed successfully");
    Ok(result)
}

/// Execute PITR recovery
#[allow(clippy::too_many_arguments)]
pub async fn pitr_restore(
    target_time: String,
    target_dir: PathBuf,
    backup_dir: PathBuf,
    wal_archive_dir: Option<PathBuf>,
    storage_opts: PitrStorageOptions,
    auto_start: bool,
    pg_bin_dir: Option<PathBuf>,
    yes: bool,
) -> Result<()> {
    info!("[pitr-restore] Starting PITR recovery to: {}", target_time);
    info!("[pitr-restore] Target directory: {:?}", target_dir);

    let remote_storage = if storage_opts.remote_storage {
        Some(create_storage(&storage_opts).await?)
    } else {
        None
    };

    // Parse target time
    let target = RecoveryTarget::parse(&target_time)
        .map_err(|e| anyhow!("Invalid target time '{}': {}", target_time, e))?;

    // Create planner
    let mut planner = PitrPlanner::new(backup_dir.clone());

    if let Some(wal_dir) = &wal_archive_dir {
        planner = planner.with_wal_archive_dir(wal_dir.clone());
    }

    planner = planner.with_wal_prefix(storage_opts.wal_prefix.clone());

    // Create storage for planner if remote storage is enabled
    if let Some(storage) = remote_storage.clone() {
        planner = planner.with_storage(storage);
    }

    // Compute the plan
    info!("[pitr-restore] Computing recovery plan...");
    let plan = planner
        .plan_recovery(target)
        .await
        .map_err(|e| anyhow!("Failed to compute recovery plan: {}", e))?;

    if !plan.validation.is_valid {
        error!("[pitr-restore] Recovery plan is invalid:");
        for err in &plan.validation.errors {
            error!("  - {}", err);
        }
        return Err(anyhow!(
            "Recovery plan is invalid: {}",
            plan.validation.errors.join("; ")
        ));
    }

    // Show plan summary and confirm
    println!("\n=== PITR Recovery Plan ===\n");
    println!("Target Time:       {}", target_time);
    println!("Target Directory:  {:?}", target_dir);
    println!(
        "Base Backup:       {} ({})",
        plan.base_backup.id, plan.base_backup.start_time
    );
    println!("WAL Segments:      {}", plan.wal_segments.len());
    println!(
        "Est. Download:     {:.2} MB",
        plan.estimated_download_bytes as f64 / 1024.0 / 1024.0
    );
    println!("Auto-start:        {}", auto_start);
    println!();

    if !plan.validation.warnings.is_empty() {
        println!("Warnings:");
        for warn in &plan.validation.warnings {
            println!("  ⚠ {}", warn);
        }
        println!();
    }

    // Confirm unless --yes is specified
    if !yes {
        let confirmed = Confirm::new()
            .with_prompt(
                "This will restore the database to the target directory.\nAny existing data in the target directory will be removed.\nProceed with recovery?",
            )
            .default(false)
            .interact()?;

        if !confirmed {
            println!("Recovery cancelled.");
            return Ok(());
        }
    }

    // Create executor
    let mut executor = PitrExecutor::new(plan, target_dir.clone())
        .with_backup_dir(backup_dir)
        .with_auto_start(auto_start);

    // Create storage for executor if remote storage is enabled
    if let Some(storage) = remote_storage {
        executor = executor.with_storage(storage);
    }

    if let Some(pg_dir) = pg_bin_dir {
        executor = executor.with_pg_bin_dir(pg_dir);
    }

    // Execute recovery
    info!("[pitr-restore] Executing recovery...");
    let result = executor
        .execute()
        .await
        .map_err(|e| anyhow!("Recovery failed: {}", e))?;

    println!("\n=== Recovery Complete ===\n");
    println!("Result ID:         {}", result.id);
    println!("Status:            {:?}", result.status);
    println!("Target Directory:  {:?}", result.target_dir);
    println!(
        "Duration:          {} seconds",
        result
            .completed_at
            .map(|t| (t - result.started_at).num_seconds())
            .unwrap_or(0)
    );
    println!();
    println!("Details:");
    println!(
        "  WAL Downloaded:  {}",
        result.details.wal_segments_downloaded
    );
    println!(
        "  Bytes Downloaded: {:.2} MB",
        result.details.bytes_downloaded as f64 / 1024.0 / 1024.0
    );
    println!("  Recovery Mode:   {}", result.details.recovery_mode);
    println!();

    if auto_start {
        println!("PostgreSQL has been started in recovery mode.");
        println!("Monitor the logs at: {:?}/postgresql.log", target_dir);
    } else {
        println!("To complete recovery:");
        println!(
            "  1. Start PostgreSQL with: pg_ctl start -D {:?}",
            target_dir
        );
        println!("  2. PostgreSQL will replay WAL and pause at the target time");
        println!("  3. Verify the data, then promote: SELECT pg_wal_replay_resume();");
    }

    info!("[pitr-restore] Recovery completed successfully");
    Ok(())
}

/// List available recovery options
#[allow(clippy::too_many_arguments)]
pub async fn pitr_list(
    backup_dir: PathBuf,
    wal_archive_dir: Option<PathBuf>,
    storage_opts: PitrStorageOptions,
    format: String,
) -> Result<()> {
    info!("[pitr-list] Listing recovery options...");

    // Create planner
    let mut planner = PitrPlanner::new(backup_dir);

    if let Some(wal_dir) = wal_archive_dir {
        planner = planner.with_wal_archive_dir(wal_dir);
    }

    planner = planner.with_wal_prefix(storage_opts.wal_prefix.clone());

    if storage_opts.remote_storage {
        let storage = create_storage(&storage_opts).await?;
        planner = planner.with_storage(storage);
    }

    // Get recovery options
    let options = planner
        .list_recovery_options()
        .await
        .map_err(|e| anyhow!("Failed to list recovery options: {}", e))?;

    match format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&serde_json::json!({
                "backups": options.available_backups.iter().map(|b| {
                    serde_json::json!({
                        "id": b.id.to_string(),
                        "start_time": b.start_time.to_rfc3339(),
                        "end_time": b.end_time.map(|t| t.to_rfc3339()),
                        "server_version": b.server_version,
                        "size_bytes": b.size_bytes,
                        "is_remote": b.is_remote,
                        "wal_start": b.wal_start,
                        "wal_end": b.wal_end,
                    })
                }).collect::<Vec<_>>(),
                "wal_coverage": {
                    "segment_count": options.wal_coverage.segment_count,
                    "total_size_bytes": options.wal_coverage.total_size_bytes,
                    "earliest_lsn": options.wal_coverage.earliest_lsn,
                    "latest_lsn": options.wal_coverage.latest_lsn,
                    "earliest_time": options.wal_coverage.earliest_time.map(|t| t.to_rfc3339()),
                    "latest_time": options.wal_coverage.latest_time.map(|t| t.to_rfc3339()),
                    "timelines": options.wal_coverage.timelines,
                    "gaps": options.wal_coverage.gaps.len(),
                },
                "recovery_window": {
                    "earliest": options.earliest_recoverable.map(|t| t.to_rfc3339()),
                    "latest": options.latest_recoverable.map(|t| t.to_rfc3339()),
                }
            }))?;
            println!("{}", json);
        }
        _ => {
            println!("\n=== Available Recovery Options ===\n");

            // Backups
            println!("Base Backups ({}):", options.available_backups.len());
            if options.available_backups.is_empty() {
                println!("  (none found)");
            } else {
                println!(
                    "  {:<36}  {:<24}  {:<10}  {:<8}",
                    "ID", "Start Time", "Size (MB)", "Remote"
                );
                println!("  {}", "-".repeat(82));
                for backup in &options.available_backups {
                    println!(
                        "  {:<36}  {:<24}  {:>10.2}  {:<8}",
                        backup.id,
                        backup.start_time.format("%Y-%m-%d %H:%M:%S"),
                        backup.size_bytes as f64 / 1024.0 / 1024.0,
                        if backup.is_remote { "yes" } else { "no" }
                    );
                }
            }
            println!();

            // WAL Coverage
            println!("WAL Coverage:");
            println!("  Segments:        {}", options.wal_coverage.segment_count);
            println!(
                "  Total Size:      {:.2} MB",
                options.wal_coverage.total_size_bytes as f64 / 1024.0 / 1024.0
            );
            if let Some(earliest) = &options.wal_coverage.earliest_lsn {
                println!("  Earliest LSN:    {}", earliest);
            }
            if let Some(latest) = &options.wal_coverage.latest_lsn {
                println!("  Latest LSN:      {}", latest);
            }
            if let Some(earliest) = options.wal_coverage.earliest_time {
                println!("  Earliest Time:   {}", earliest);
            }
            if let Some(latest) = options.wal_coverage.latest_time {
                println!("  Latest Time:     {}", latest);
            }
            println!("  Timelines:       {:?}", options.wal_coverage.timelines);
            if !options.wal_coverage.gaps.is_empty() {
                println!(
                    "  Gaps:            {} (recovery may fail if target falls in a gap)",
                    options.wal_coverage.gaps.len()
                );
            }
            println!();

            // Recovery Window
            println!("Recovery Window:");
            if let Some(earliest) = options.earliest_recoverable {
                println!("  Earliest:        {}", earliest);
            } else {
                println!("  Earliest:        (no backups available)");
            }
            if let Some(latest) = options.latest_recoverable {
                println!("  Latest:          {}", latest);
            } else {
                println!("  Latest:          (no WAL coverage)");
            }
            println!();
        }
    }

    info!("[pitr-list] Recovery options listed successfully");
    Ok(())
}

/// Create storage provider from options
async fn create_storage(opts: &PitrStorageOptions) -> Result<PostgresBackupStorage> {
    let bucket = opts
        .bucket
        .clone()
        .ok_or_else(|| anyhow!("Storage bucket is required for remote storage"))?;

    PostgresBackupStorage::new(
        opts.provider_type,
        bucket,
        opts.prefix.clone(),
        opts.region.clone(),
        opts.endpoint.clone(),
        opts.access_key.clone(),
        opts.secret_key.clone(),
        None,
        None,
        None,
    )
    .await
    .map_err(|e| anyhow!("Failed to create storage provider: {}", e))
}
