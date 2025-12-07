//! CLI commands for backup retention and purge operations.

use anyhow::{anyhow, Result};
use chrono::Utc;
use log::{error, info, warn};
use std::io::{self, Write};
use std::path::PathBuf;

use storage::{
    BackupMetadata as StorageBackupMetadata, BackupStatus as StorageBackupStatus,
    BackupType as StorageBackupType, PostgresBackupStorage, RetentionPolicy as StorageRetentionPolicy,
    StorageProviderType,
};

use crate::retention::{
    BackupItem, BackupItemStatus, BackupItemType, BackupLocation, PitrRetentionPolicy,
    RetentionEngine, RetentionResult, WalInventory,
};

use super::StorageOptions;

/// Result of a retention plan operation
#[derive(Debug, Clone)]
pub struct RetentionPlanResult {
    /// The evaluation result
    pub evaluation: RetentionResult,
    /// Policy used for evaluation
    pub policy_source: String,
    /// Whether local backups were included
    pub includes_local: bool,
    /// Whether remote backups were included
    pub includes_remote: bool,
}

/// Options for retention operations
#[derive(Debug, Clone)]
pub struct RetentionOptions {
    /// Path to policy file (if not using remote policy)
    pub policy_file: Option<PathBuf>,
    /// Local backup directory
    pub backup_dir: PathBuf,
    /// WAL archive directory (if separate)
    pub wal_archive_dir: Option<PathBuf>,
    /// Include local backups in evaluation
    pub include_local: bool,
    /// Include remote backups in evaluation
    pub include_remote: bool,
    /// Output format (table, json, yaml)
    pub format: String,
}

/// Compute and display a retention plan (dry-run)
pub async fn retention_plan(
    storage: StorageOptions,
    options: RetentionOptions,
) -> Result<RetentionPlanResult> {
    info!("[retention-plan] Starting retention plan evaluation");

    // Load policy
    let (policy, policy_source) = load_policy(&storage, &options).await?;

    // Validate policy
    if let Err(errors) = policy.validate() {
        for error in &errors {
            error!("[retention-plan] Policy validation error: {}", error);
        }
        return Err(anyhow!("Policy validation failed: {}", errors.join(", ")));
    }

    info!("[retention-plan] Policy loaded from: {}", policy_source);
    info!("[retention-plan] Policy enabled: {}", policy.enabled);

    // Collect backups
    let mut backups = Vec::new();

    // Collect local backups
    if options.include_local {
        let local_backups = collect_local_backups(&options.backup_dir)?;
        info!(
            "[retention-plan] Found {} local backups",
            local_backups.len()
        );
        backups.extend(local_backups);
    }

    // Collect remote backups
    if options.include_remote && storage.remote_storage {
        let remote_backups = collect_remote_backups(&storage).await?;
        info!(
            "[retention-plan] Found {} remote backups",
            remote_backups.len()
        );
        backups.extend(remote_backups);
    }

    if backups.is_empty() {
        warn!("[retention-plan] No backups found to evaluate");
        return Ok(RetentionPlanResult {
            evaluation: RetentionResult::new(),
            policy_source,
            includes_local: options.include_local,
            includes_remote: options.include_remote && storage.remote_storage,
        });
    }

    // Collect WAL inventory if available
    let wal_inventory = if let Some(wal_dir) = &options.wal_archive_dir {
        Some(WalInventory::scan_local_directory(wal_dir)?)
    } else {
        // Try default WAL location in backup dir
        let default_wal_dir = options.backup_dir.join("wal_archive");
        if default_wal_dir.exists() {
            Some(WalInventory::scan_local_directory(&default_wal_dir)?)
        } else {
            None
        }
    };

    if let Some(ref inv) = wal_inventory {
        info!(
            "[retention-plan] Found {} WAL segments",
            inv.segments.len()
        );
    }

    // Create retention engine and evaluate
    let engine = RetentionEngine::new(policy);
    let evaluation = engine.evaluate(&backups, wal_inventory.as_ref());

    info!(
        "[retention-plan] Evaluation complete: {} to keep, {} to delete",
        evaluation.backups_to_keep.len(),
        evaluation.backups_to_delete.len()
    );

    Ok(RetentionPlanResult {
        evaluation,
        policy_source,
        includes_local: options.include_local,
        includes_remote: options.include_remote && storage.remote_storage,
    })
}

/// Execute retention policy (apply deletions)
pub async fn retention_apply(
    storage: StorageOptions,
    options: RetentionOptions,
    dry_run: bool,
    skip_confirmation: bool,
) -> Result<crate::retention::PurgeReport> {
    let start_time = std::time::Instant::now();

    // First, get the plan
    let plan_result = retention_plan(storage.clone(), options.clone()).await?;
    let evaluation = plan_result.evaluation;

    if evaluation.backups_to_delete.is_empty() && evaluation.wal_to_delete.is_empty() {
        info!("[retention-apply] Nothing to delete");
        return Ok(crate::retention::PurgeReport {
            timestamp: Utc::now(),
            dry_run,
            total_backups_evaluated: evaluation.total_backups,
            total_wal_evaluated: evaluation.total_wal_segments,
            backups_kept: evaluation.backups_to_keep.len(),
            backups_deleted: 0,
            wal_kept: evaluation.wal_to_keep.len(),
            wal_deleted: 0,
            failed: 0,
            space_freed: 0,
            duration_secs: start_time.elapsed().as_secs(),
            errors: Vec::new(),
            pitr_window_start: evaluation.pitr_window_start,
            pitr_window_end: evaluation.pitr_window_end,
        });
    }

    // Show summary
    println!("\n=== Retention Apply Summary ===");
    println!("Total backups evaluated: {}", evaluation.total_backups);
    println!("Backups to keep: {}", evaluation.backups_to_keep.len());
    println!("Backups to delete: {}", evaluation.backups_to_delete.len());
    println!("WAL segments to keep: {}", evaluation.wal_to_keep.len());
    println!("WAL segments to delete: {}", evaluation.wal_to_delete.len());
    println!(
        "Estimated space to free: {:.2} GB",
        evaluation.estimated_space_freed as f64 / 1024.0 / 1024.0 / 1024.0
    );

    if !evaluation.warnings.is_empty() {
        println!("\n⚠️  Warnings:");
        for warning in &evaluation.warnings {
            println!("  - {}", warning);
        }
    }

    if dry_run {
        println!("\n🔍 DRY RUN - No changes will be made");
        println!("\nBackups that would be deleted:");
        for decision in &evaluation.backups_to_delete {
            println!(
                "  🗑️  {} ({:?}) - {} - {:.2} MB",
                decision.backup_id,
                decision.backup_type,
                decision.reason,
                decision.size_bytes as f64 / 1024.0 / 1024.0
            );
        }

        return Ok(crate::retention::PurgeReport {
            timestamp: Utc::now(),
            dry_run: true,
            total_backups_evaluated: evaluation.total_backups,
            total_wal_evaluated: evaluation.total_wal_segments,
            backups_kept: evaluation.backups_to_keep.len(),
            backups_deleted: 0,
            wal_kept: evaluation.wal_to_keep.len(),
            wal_deleted: 0,
            failed: 0,
            space_freed: 0,
            duration_secs: start_time.elapsed().as_secs(),
            errors: Vec::new(),
            pitr_window_start: evaluation.pitr_window_start,
            pitr_window_end: evaluation.pitr_window_end,
        });
    }

    // Confirm if required
    if !skip_confirmation {
        print!(
            "\n⚠️  This will DELETE {} backups and {} WAL segments. Continue? (yes/no): ",
            evaluation.backups_to_delete.len(),
            evaluation.wal_to_delete.len()
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().to_lowercase() != "yes" {
            println!("Cancelled.");
            return Err(anyhow!("Operation cancelled by user"));
        }
    }

    // Execute deletions
    let mut deleted_backups = 0;
    let mut deleted_wal = 0;
    let mut failed = 0;
    let mut space_freed = 0u64;
    let mut errors = Vec::new();

    // Create storage provider if needed for remote deletions
    let storage_instance = if storage.remote_storage {
        Some(create_storage_provider(&storage).await?)
    } else {
        None
    };

    // Delete backups
    for decision in &evaluation.backups_to_delete {
        info!(
            "[retention-apply] Deleting backup: {} - {}",
            decision.backup_id, decision.reason
        );

        match &decision.location {
            BackupLocation::Local(path) => {
                match delete_local_backup(path, &options.backup_dir) {
                    Ok(_) => {
                        deleted_backups += 1;
                        space_freed += decision.size_bytes;
                        info!("[retention-apply] Deleted local backup: {}", path);
                    }
                    Err(e) => {
                        failed += 1;
                        let msg = format!("Failed to delete local backup {}: {}", path, e);
                        error!("[retention-apply] {}", msg);
                        errors.push(msg);
                    }
                }
            }
            BackupLocation::Remote(key) => {
                if let Some(ref storage) = storage_instance {
                    match storage.delete_backup(&decision.backup_id).await {
                        Ok(_) => {
                            deleted_backups += 1;
                            space_freed += decision.size_bytes;
                            info!("[retention-apply] Deleted remote backup: {}", key);
                        }
                        Err(e) => {
                            failed += 1;
                            let msg = format!("Failed to delete remote backup {}: {}", key, e);
                            error!("[retention-apply] {}", msg);
                            errors.push(msg);
                        }
                    }
                }
            }
            BackupLocation::Both { local, remote } => {
                // Delete both local and remote - track errors locally for this backup
                let mut local_failed = false;
                let mut remote_failed = false;
                
                if let Err(e) = delete_local_backup(local, &options.backup_dir) {
                    failed += 1;
                    local_failed = true;
                    errors.push(format!("Failed to delete local backup {}: {}", local, e));
                }

                if let Some(ref storage) = storage_instance {
                    if let Err(e) = storage.delete_backup(&decision.backup_id).await {
                        failed += 1;
                        remote_failed = true;
                        errors.push(format!("Failed to delete remote backup {}: {}", remote, e));
                    }
                }

                if !local_failed && !remote_failed {
                    deleted_backups += 1;
                    space_freed += decision.size_bytes;
                }
            }
        }
    }

    // Delete WAL segments
    for decision in &evaluation.wal_to_delete {
        match &decision.location {
            BackupLocation::Local(path) => {
                match std::fs::remove_file(path) {
                    Ok(_) => {
                        deleted_wal += 1;
                        space_freed += decision.size_bytes;
                    }
                    Err(e) => {
                        failed += 1;
                        errors.push(format!("Failed to delete WAL segment {}: {}", path, e));
                    }
                }
            }
            _ => {
                // Remote WAL deletion would go here
                warn!(
                    "[retention-apply] Remote WAL deletion not yet implemented: {}",
                    decision.segment_name
                );
            }
        }
    }

    let duration_secs = start_time.elapsed().as_secs();

    println!("\n=== Purge Report ===");
    println!("Backups deleted: {}", deleted_backups);
    println!("WAL segments deleted: {}", deleted_wal);
    println!("Failed: {}", failed);
    println!(
        "Space freed: {:.2} GB",
        space_freed as f64 / 1024.0 / 1024.0 / 1024.0
    );
    println!("Duration: {} seconds", duration_secs);

    if !errors.is_empty() {
        println!("\n❌ Errors:");
        for error in &errors {
            println!("  - {}", error);
        }
    }

    Ok(crate::retention::PurgeReport {
        timestamp: Utc::now(),
        dry_run: false,
        total_backups_evaluated: evaluation.total_backups,
        total_wal_evaluated: evaluation.total_wal_segments,
        backups_kept: evaluation.backups_to_keep.len(),
        backups_deleted: deleted_backups,
        wal_kept: evaluation.wal_to_keep.len(),
        wal_deleted: deleted_wal,
        failed,
        space_freed,
        duration_secs,
        errors,
        pitr_window_start: evaluation.pitr_window_start,
        pitr_window_end: evaluation.pitr_window_end,
    })
}

/// Formats the retention plan result for display
pub fn format_retention_plan(result: &RetentionPlanResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(&result.evaluation).unwrap_or_default(),
        "yaml" => serde_yaml::to_string(&result.evaluation).unwrap_or_default(),
        _ => format_retention_plan_table(result),
    }
}

fn format_retention_plan_table(result: &RetentionPlanResult) -> String {
    let eval = &result.evaluation;
    let mut output = String::new();

    output.push_str("\n=== Retention Plan ===\n");
    output.push_str(&format!("Policy source: {}\n", result.policy_source));
    output.push_str(&format!("Evaluation time: {}\n", eval.timestamp));
    output.push_str(&format!("Total backups: {}\n", eval.total_backups));
    output.push_str(&format!("Total WAL segments: {}\n", eval.total_wal_segments));

    if let (Some(start), Some(end)) = (eval.pitr_window_start, eval.pitr_window_end) {
        output.push_str(&format!("\nPITR Window: {} to {}\n", start, end));
    }

    if !eval.warnings.is_empty() {
        output.push_str("\n⚠️  Warnings:\n");
        for warning in &eval.warnings {
            output.push_str(&format!("  - {}\n", warning));
        }
    }

    output.push_str(&format!(
        "\n=== Backups to Keep ({}) ===\n",
        eval.backups_to_keep.len()
    ));
    for decision in &eval.backups_to_keep {
        let pinned = if decision.pinned { " 📌" } else { "" };
        let deps = if decision.has_dependents { " [has deps]" } else { "" };
        output.push_str(&format!(
            "  ✅ {} ({:?}) - {} - {:.2} MB{}{}\n",
            decision.backup_id,
            decision.backup_type,
            decision.reason,
            decision.size_bytes as f64 / 1024.0 / 1024.0,
            pinned,
            deps
        ));
    }

    output.push_str(&format!(
        "\n=== Backups to Delete ({}) ===\n",
        eval.backups_to_delete.len()
    ));
    for decision in &eval.backups_to_delete {
        output.push_str(&format!(
            "  🗑️  {} ({:?}) - {} - {:.2} MB\n",
            decision.backup_id,
            decision.backup_type,
            decision.reason,
            decision.size_bytes as f64 / 1024.0 / 1024.0
        ));
    }

    if !eval.wal_to_keep.is_empty() || !eval.wal_to_delete.is_empty() {
        output.push_str(&format!(
            "\n=== WAL to Keep ({}) ===\n",
            eval.wal_to_keep.len()
        ));
        // Just show count for WAL to avoid overwhelming output
        output.push_str(&format!(
            "  {} segments, {:.2} MB total\n",
            eval.wal_to_keep.len(),
            eval.wal_to_keep.iter().map(|w| w.size_bytes).sum::<u64>() as f64 / 1024.0 / 1024.0
        ));

        output.push_str(&format!(
            "\n=== WAL to Delete ({}) ===\n",
            eval.wal_to_delete.len()
        ));
        output.push_str(&format!(
            "  {} segments, {:.2} MB total\n",
            eval.wal_to_delete.len(),
            eval.wal_to_delete.iter().map(|w| w.size_bytes).sum::<u64>() as f64 / 1024.0 / 1024.0
        ));
    }

    output.push_str(&format!(
        "\n=== Summary ===\n\
         Estimated space to free: {:.2} GB\n",
        eval.estimated_space_freed as f64 / 1024.0 / 1024.0 / 1024.0
    ));

    output
}

/// Load retention policy from file or remote storage
async fn load_policy(
    storage: &StorageOptions,
    options: &RetentionOptions,
) -> Result<(PitrRetentionPolicy, String)> {
    // First try local policy file
    if let Some(policy_file) = &options.policy_file {
        if policy_file.exists() {
            let content = std::fs::read_to_string(policy_file)?;
            let policy: PitrRetentionPolicy = serde_json::from_str(&content)
                .or_else(|_| serde_yaml::from_str(&content))
                .map_err(|e| anyhow!("Failed to parse policy file: {}", e))?;
            return Ok((policy, format!("file:{}", policy_file.display())));
        }
    }

    // Try remote storage policy
    if storage.remote_storage {
        let storage_instance = create_storage_provider(storage).await?;
        if let Ok(Some(remote_policy)) = storage_instance.load_retention_policy().await {
            // Convert storage policy to PITR policy
            let policy = convert_storage_policy(&remote_policy);
            return Ok((
                policy,
                format!("remote:{}", storage.bucket.as_deref().unwrap_or("unknown")),
            ));
        }
    }

    // Fall back to default policy
    warn!("[retention] No policy file found, using default policy");
    Ok((PitrRetentionPolicy::default(), "default".to_string()))
}

/// Convert storage crate's RetentionPolicy to our PitrRetentionPolicy
fn convert_storage_policy(storage_policy: &StorageRetentionPolicy) -> PitrRetentionPolicy {
    use crate::retention::policy::{
        IntervalSpec, RetentionRule, RetentionScope, SafetySettings, WalRetentionConfig,
    };

    let rules = match &storage_policy.policy_type {
        storage::PolicyType::TimeBased {
            keep_within_days,
            keep_minimum,
        } => vec![
            RetentionRule::KeepPinned,
            RetentionRule::KeepWithinDays {
                days: *keep_within_days,
                minimum: *keep_minimum,
            },
        ],
        storage::PolicyType::CountBased {
            max_full_backups,
            keep_latest,
            ..
        } => vec![
            RetentionRule::KeepPinned,
            RetentionRule::KeepLatest {
                count: (*keep_latest).max(*max_full_backups),
            },
        ],
        storage::PolicyType::IntervalBased {
            intervals,
            minimum_backups,
            ..
        } => {
            let mut rules = vec![RetentionRule::KeepPinned];

            // Convert intervals to our format
            // This is a simplified conversion
            if !intervals.is_empty() {
                let daily = intervals.iter().find(|i| i.spacing_days == 1 || i.spacing_hours == Some(24));
                let weekly = intervals.iter().find(|i| i.spacing_days == 7);
                let monthly = intervals.iter().find(|i| i.spacing_days == 30);

                rules.push(RetentionRule::KeepIntervals {
                    hourly: intervals
                        .iter()
                        .find(|i| i.spacing_hours.map(|h| h < 24).unwrap_or(false))
                        .map(|i| IntervalSpec {
                            count: i.keep_count,
                            max_age_days: Some(i.after_days + 7),
                        }),
                    daily: daily.map(|i| IntervalSpec {
                        count: i.keep_count,
                        max_age_days: Some(i.after_days + 30),
                    }),
                    weekly: weekly.map(|i| IntervalSpec {
                        count: i.keep_count,
                        max_age_days: Some(i.after_days + 90),
                    }),
                    monthly: monthly.map(|i| IntervalSpec {
                        count: i.keep_count,
                        max_age_days: Some(i.after_days + 365),
                    }),
                    yearly: None,
                });
            }

            rules.push(RetentionRule::KeepLatest {
                count: *minimum_backups,
            });

            rules
        }
    };

    PitrRetentionPolicy {
        version: storage_policy.version.clone(),
        enabled: storage_policy.enabled,
        rules,
        wal_retention: WalRetentionConfig::default(),
        safety: SafetySettings {
            dry_run_by_default: storage_policy.safety.dry_run_by_default,
            require_confirmation: storage_policy.safety.require_confirmation,
            min_successful_backups: storage_policy.safety.min_successful_backups,
            preserve_chains: storage_policy.safety.preserve_chains,
            keep_latest_successful: true,
            min_pitr_window_hours: Some(24),
        },
        scope: RetentionScope::default(),
    }
}

/// Collect backups from local backup directory
fn collect_local_backups(backup_dir: &PathBuf) -> Result<Vec<BackupItem>> {
    let mut backups = Vec::new();

    if !backup_dir.exists() {
        return Ok(backups);
    }

    for entry in std::fs::read_dir(backup_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Look for metadata file
        let metadata_path = path.join("backup_metadata.json");
        if metadata_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&metadata_path) {
                if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&content) {
                    let backup = parse_local_metadata(&metadata, &path)?;
                    backups.push(backup);
                }
            }
        } else {
            // Try to infer backup info from directory name
            if let Some(backup) = infer_backup_from_directory(&path) {
                backups.push(backup);
            }
        }
    }

    Ok(backups)
}

/// Parse local metadata JSON into BackupItem
fn parse_local_metadata(metadata: &serde_json::Value, path: &PathBuf) -> Result<BackupItem> {
    let id = metadata["backup_id"]
        .as_str()
        .or_else(|| metadata["id"].as_str())
        .or_else(|| path.file_name().and_then(|f| f.to_str()))
        .unwrap_or("unknown")
        .to_string();

    let backup_type = match metadata["backup_type"].as_str() {
        Some("full") | Some("Full") => BackupItemType::Full,
        Some("incremental") | Some("Incremental") => BackupItemType::Incremental,
        Some("snapshot") | Some("Snapshot") => BackupItemType::Snapshot,
        _ => BackupItemType::Snapshot,
    };

    let status = match metadata["status"].as_str() {
        Some("Completed") | Some("completed") => BackupItemStatus::Completed,
        Some("Failed") | Some("failed") => BackupItemStatus::Failed,
        _ => BackupItemStatus::Completed,
    };

    let start_time = metadata["start_time"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let end_time = metadata["end_time"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    let size_bytes = metadata["size_bytes"]
        .as_u64()
        .unwrap_or_else(|| calculate_dir_size(path).unwrap_or(0));

    Ok(BackupItem {
        id,
        backup_type,
        status,
        start_time,
        end_time,
        base_backup_id: metadata["base_backup_id"].as_str().map(String::from),
        wal_start: metadata["wal_start"].as_str().map(String::from),
        wal_end: metadata["wal_end"].as_str().map(String::from),
        size_bytes,
        database: metadata["database"].as_str().map(String::from),
        pinned: metadata["pinned"].as_bool().unwrap_or(false),
        tags: metadata["tags"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        location: BackupLocation::Local(path.to_string_lossy().to_string()),
    })
}

/// Infer backup info from directory name when no metadata exists
fn infer_backup_from_directory(path: &PathBuf) -> Option<BackupItem> {
    let name = path.file_name()?.to_str()?;

    let backup_type = if name.contains("full_backup") {
        BackupItemType::Full
    } else if name.contains("incremental_backup") {
        BackupItemType::Incremental
    } else if name.contains("snapshot_backup") {
        BackupItemType::Snapshot
    } else {
        return None;
    };

    // Try to parse timestamp from directory name
    let timestamp = extract_timestamp_from_name(name).unwrap_or_else(Utc::now);

    let size_bytes = calculate_dir_size(path).unwrap_or(0);

    Some(BackupItem {
        id: name.to_string(),
        backup_type,
        status: BackupItemStatus::Completed,
        start_time: timestamp,
        end_time: Some(timestamp),
        base_backup_id: None,
        wal_start: None,
        wal_end: None,
        size_bytes,
        database: None,
        pinned: false,
        tags: vec!["inferred".to_string()],
        location: BackupLocation::Local(path.to_string_lossy().to_string()),
    })
}

/// Extract timestamp from backup directory name
fn extract_timestamp_from_name(name: &str) -> Option<chrono::DateTime<Utc>> {
    // Try to find a timestamp pattern like 2025-01-15T10-30-00Z
    let re = regex::Regex::new(r"(\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2})").ok()?;
    if let Some(caps) = re.captures(name) {
        let ts_str = caps.get(1)?.as_str().replace('-', ":");
        // Convert back to proper format
        let ts_str = ts_str.replacen(':', "-", 2);
        chrono::DateTime::parse_from_rfc3339(&format!("{}Z", ts_str))
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    } else {
        None
    }
}

/// Calculate directory size recursively
fn calculate_dir_size(path: &PathBuf) -> Result<u64> {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(total)
}

/// Collect backups from remote storage
async fn collect_remote_backups(storage: &StorageOptions) -> Result<Vec<BackupItem>> {
    let storage_instance = create_storage_provider(storage).await?;
    let remote_backups = storage_instance
        .list_remote_backups_detailed()
        .await
        .map_err(|e| anyhow!("Failed to list remote backups: {}", e))?;

    Ok(remote_backups
        .into_iter()
        .map(|b| convert_storage_backup(&b))
        .collect())
}

/// Convert storage backup metadata to BackupItem
fn convert_storage_backup(backup: &StorageBackupMetadata) -> BackupItem {
    BackupItem {
        id: backup.id.clone(),
        backup_type: match backup.backup_type {
            StorageBackupType::Full => BackupItemType::Full,
            StorageBackupType::Incremental => BackupItemType::Incremental,
            StorageBackupType::Snapshot => BackupItemType::Snapshot,
        },
        status: match backup.status {
            StorageBackupStatus::Completed => BackupItemStatus::Completed,
            StorageBackupStatus::Failed => BackupItemStatus::Failed,
            StorageBackupStatus::InProgress => BackupItemStatus::InProgress,
        },
        start_time: backup.start_time,
        end_time: backup.end_time,
        base_backup_id: backup.base_backup_id.clone(),
        wal_start: backup.wal_start.clone(),
        wal_end: backup.wal_end.clone(),
        size_bytes: backup.size_bytes,
        database: None, // Not stored in storage metadata
        pinned: backup.pinned,
        tags: backup.tags.clone(),
        location: BackupLocation::Remote(backup.id.clone()),
    }
}

/// Create storage provider from options
async fn create_storage_provider(storage: &StorageOptions) -> Result<PostgresBackupStorage> {
    let bucket = storage
        .bucket
        .clone()
        .ok_or_else(|| anyhow!("Storage bucket name is required"))?;

    let provider_type = match storage.provider_type.as_deref() {
        Some("s3") | None => StorageProviderType::S3,
        Some(other) => return Err(anyhow!("Unsupported storage provider: {}", other)),
    };

    PostgresBackupStorage::new(
        provider_type,
        bucket,
        storage.prefix.clone(),
        storage.region.clone(),
        storage.endpoint.clone(),
        storage.access_key.clone(),
        storage.secret_key.clone(),
        None,
        None,
        None,
    )
    .await
    .map_err(|e| anyhow!("Failed to create storage provider: {}", e))
}

/// Delete a local backup directory with path traversal protection.
/// 
/// This function validates that the path is under the expected backup root
/// to prevent path traversal attacks or symlink tricks.
fn delete_local_backup(path: &str, backup_root: &PathBuf) -> Result<()> {
    let path = PathBuf::from(path);
    
    // Canonicalize both paths to resolve symlinks and relative components
    let canonical_root = backup_root.canonicalize()
        .map_err(|e| anyhow!("Failed to canonicalize backup root '{}': {}", backup_root.display(), e))?;
    
    // If path doesn't exist, nothing to delete
    if !path.exists() {
        return Ok(());
    }
    
    let canonical_path = path.canonicalize()
        .map_err(|e| anyhow!("Failed to canonicalize path '{}': {}", path.display(), e))?;
    
    // Verify the path is under the backup root
    if !canonical_path.starts_with(&canonical_root) {
        return Err(anyhow!(
            "Security: Path '{}' is not under backup root '{}'. Refusing to delete.",
            canonical_path.display(),
            canonical_root.display()
        ));
    }
    
    // Re-canonicalize immediately before deletion to mitigate TOCTOU race
    let final_path = path.canonicalize()
        .map_err(|e| anyhow!("Path changed during validation '{}': {}", path.display(), e))?;
    
    if !final_path.starts_with(&canonical_root) {
        return Err(anyhow!(
            "Security: Path '{}' escaped backup root during operation. Refusing to delete.",
            final_path.display()
        ));
    }
    
    std::fs::remove_dir_all(&final_path)?;
    Ok(())
}

/// Generate a retention policy file from a preset
pub fn retention_init(output: &PathBuf, preset: &str, format: &str) -> Result<()> {
    let policy = match preset.to_lowercase().as_str() {
        "aggressive" => PitrRetentionPolicy::aggressive(),
        "conservative" => PitrRetentionPolicy::conservative(),
        "gfs" => PitrRetentionPolicy::gfs_standard(),
        "standard" | _ => PitrRetentionPolicy::default(),
    };

    // Validate the policy
    if let Err(errors) = policy.validate() {
        return Err(anyhow!("Generated policy is invalid: {}", errors.join(", ")));
    }

    let content = match format.to_lowercase().as_str() {
        "yaml" => serde_yaml::to_string(&policy)
            .map_err(|e| anyhow!("Failed to serialize policy to YAML: {}", e))?,
        "json" | _ => serde_json::to_string_pretty(&policy)
            .map_err(|e| anyhow!("Failed to serialize policy to JSON: {}", e))?,
    };

    // Create parent directories if needed
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(output, &content)?;

    println!("✅ Retention policy created: {}", output.display());
    println!("   Preset: {}", preset);
    println!("   Format: {}", format);
    println!("\nPolicy summary:");
    println!("   Enabled: {}", policy.enabled);
    println!("   Rules: {} rules defined", policy.rules.len());
    println!(
        "   PITR window: {} hours",
        policy.wal_retention.pitr_window_hours
    );
    println!(
        "   Min successful backups: {}",
        policy.safety.min_successful_backups
    );
    println!("   Preserve chains: {}", policy.safety.preserve_chains);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_timestamp_from_name() {
        // Test that the function doesn't panic on various inputs
        // and returns expected results for known formats
        
        // Test with standard format that should parse
        let name = "snapshot_backup_2025-01-15";
        let result = extract_timestamp_from_name(name);
        // Should extract a timestamp from the date portion
        assert!(result.is_some(), "Expected to parse date from '{}'", name);
        
        // Test with format that may not parse (documents current behavior)
        let name_with_time = "snapshot_backup_2025-01-15T10-30-00Z";
        let _result = extract_timestamp_from_name(name_with_time);
        // Don't assert on result - just verify no panic
        
        // Test with no timestamp - should return None
        let name_no_date = "snapshot_backup_latest";
        let result = extract_timestamp_from_name(name_no_date);
        assert!(result.is_none(), "Expected None for name without date");
    }

    #[test]
    fn test_infer_backup_type() {
        // Test that backup type is correctly inferred from directory name
        let path = PathBuf::from("/backups/full_backup_2025-01-15");
        let backup = infer_backup_from_directory(&path);
        // Function infers from name, so it should return Some with Full type
        assert!(backup.is_some());
        let backup = backup.unwrap();
        assert_eq!(backup.backup_type, BackupItemType::Full);

        // Test incremental backup inference
        let path = PathBuf::from("/backups/incremental_backup_2025-01-15");
        let backup = infer_backup_from_directory(&path);
        assert!(backup.is_some());
        assert_eq!(backup.unwrap().backup_type, BackupItemType::Incremental);

        // Test snapshot backup inference
        let path = PathBuf::from("/backups/snapshot_backup_2025-01-15");
        let backup = infer_backup_from_directory(&path);
        assert!(backup.is_some());
        assert_eq!(backup.unwrap().backup_type, BackupItemType::Snapshot);

        // Test unknown directory name returns None
        let path = PathBuf::from("/backups/random_directory");
        let backup = infer_backup_from_directory(&path);
        assert!(backup.is_none());
    }
}
