//! Backup retention policy evaluation and purge logic

use crate::{
    BackupMetadata, BackupPurgeDecision, BackupStatus, BackupType, PolicyType, PurgeEvaluation,
    PurgeReport, RetentionPolicy, StorageError,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, HashSet};

/// Evaluates which backups should be kept or deleted based on a retention policy
pub fn evaluate_retention_policy(
    backups: &[BackupMetadata],
    policy: &RetentionPolicy,
) -> Result<PurgeEvaluation, StorageError> {
    if !policy.enabled {
        return Ok(PurgeEvaluation {
            timestamp: Utc::now(),
            total_backups: backups.len(),
            to_keep: backups
                .iter()
                .map(|b| create_keep_decision(b, "Policy is disabled", false))
                .collect(),
            to_delete: Vec::new(),
            warnings: vec!["Retention policy is disabled".to_string()],
            estimated_space_freed: 0,
        });
    }

    match &policy.policy_type {
        PolicyType::TimeBased {
            keep_within_days,
            keep_minimum,
        } => evaluate_time_based(backups, *keep_within_days, *keep_minimum, policy),
        PolicyType::CountBased {
            max_full_backups,
            max_incrementals_per_full,
            keep_latest,
        } => evaluate_count_based(
            backups,
            *max_full_backups,
            *max_incrementals_per_full,
            *keep_latest,
            policy,
        ),
        PolicyType::IntervalBased {
            intervals,
            minimum_backups,
            preserve_chains,
        } => evaluate_interval_based(
            backups,
            intervals,
            *minimum_backups,
            *preserve_chains,
            policy,
        ),
    }
}

/// Evaluates time-based retention policy
fn evaluate_time_based(
    backups: &[BackupMetadata],
    keep_within_days: u32,
    keep_minimum: usize,
    policy: &RetentionPolicy,
) -> Result<PurgeEvaluation, StorageError> {
    let now = Utc::now();
    let cutoff = now - Duration::days(keep_within_days as i64);
    let mut to_keep_ids = HashSet::new();
    let mut warnings = Vec::new();

    // Keep all completed backups within the time window
    for backup in backups
        .iter()
        .filter(|b| b.status == BackupStatus::Completed)
    {
        if backup.end_time.unwrap_or(backup.start_time) >= cutoff {
            to_keep_ids.insert(backup.id.clone());
        }
    }

    // Ensure minimum backup count
    if to_keep_ids.len() < keep_minimum {
        let mut sorted_backups: Vec<_> = backups
            .iter()
            .filter(|b| b.status == BackupStatus::Completed)
            .collect();
        sorted_backups.sort_by(|a, b| {
            b.end_time
                .unwrap_or(b.start_time)
                .cmp(&a.end_time.unwrap_or(a.start_time))
        });

        for backup in sorted_backups.iter().take(keep_minimum) {
            to_keep_ids.insert(backup.id.clone());
        }

        if to_keep_ids.len() < keep_minimum {
            warnings.push(format!(
                "Only {} completed backups available, need {} minimum",
                to_keep_ids.len(),
                keep_minimum
            ));
        }
    }

    // Always keep pinned backups
    for backup in backups.iter().filter(|b| b.pinned) {
        to_keep_ids.insert(backup.id.clone());
    }

    // Preserve chains if needed
    if policy.safety.preserve_chains {
        preserve_backup_chains(backups, &mut to_keep_ids);
    }

    build_evaluation(backups, to_keep_ids, &warnings)
}

/// Evaluates count-based retention policy
fn evaluate_count_based(
    backups: &[BackupMetadata],
    max_full_backups: usize,
    max_incrementals_per_full: usize,
    keep_latest: usize,
    policy: &RetentionPolicy,
) -> Result<PurgeEvaluation, StorageError> {
    let mut to_keep_ids = HashSet::new();
    let warnings = Vec::new();

    // Get completed backups sorted by time (newest first)
    let mut sorted_backups: Vec<_> = backups
        .iter()
        .filter(|b| b.status == BackupStatus::Completed)
        .collect();
    sorted_backups.sort_by(|a, b| {
        b.end_time
            .unwrap_or(b.start_time)
            .cmp(&a.end_time.unwrap_or(a.start_time))
    });

    // Keep the latest N backups regardless of type
    for backup in sorted_backups.iter().take(keep_latest) {
        to_keep_ids.insert(backup.id.clone());
    }

    // Keep up to max_full_backups full backups
    let full_backups: Vec<_> = sorted_backups
        .iter()
        .filter(|b| b.backup_type == BackupType::Full)
        .take(max_full_backups)
        .collect();

    for backup in &full_backups {
        to_keep_ids.insert(backup.id.clone());
    }

    // For each kept full backup, keep up to max_incrementals_per_full incrementals
    for full_backup in &full_backups {
        let incrementals: Vec<_> = sorted_backups
            .iter()
            .filter(|b| {
                b.backup_type == BackupType::Incremental
                    && b.base_backup_id.as_ref() == Some(&full_backup.id)
            })
            .take(max_incrementals_per_full)
            .collect();

        for incr in incrementals {
            to_keep_ids.insert(incr.id.clone());
        }
    }

    // Always keep pinned backups
    for backup in backups.iter().filter(|b| b.pinned) {
        to_keep_ids.insert(backup.id.clone());
    }

    // Preserve chains if needed
    if policy.safety.preserve_chains {
        preserve_backup_chains(backups, &mut to_keep_ids);
    }

    build_evaluation(backups, to_keep_ids, &warnings)
}

/// Evaluates interval-based retention policy (e.g., daily, weekly, monthly, yearly)
fn evaluate_interval_based(
    backups: &[BackupMetadata],
    intervals: &[crate::RetentionInterval],
    minimum_backups: usize,
    preserve_chains: bool,
    policy: &RetentionPolicy,
) -> Result<PurgeEvaluation, StorageError> {
    let now = Utc::now();
    let mut to_keep_ids = HashSet::new();
    let mut warnings = Vec::new();

    // Get completed backups sorted by time (newest first)
    let mut sorted_backups: Vec<_> = backups
        .iter()
        .filter(|b| b.status == BackupStatus::Completed)
        .collect();
    sorted_backups.sort_by(|a, b| {
        b.end_time
            .unwrap_or(b.start_time)
            .cmp(&a.end_time.unwrap_or(a.start_time))
    });

    // Apply each interval rule
    let mut sorted_intervals = intervals.to_vec();
    sorted_intervals.sort_by_key(|i| i.after_days);

    for (idx, interval) in sorted_intervals.iter().enumerate() {
        let interval_start = now - Duration::days(interval.after_days as i64);
        let interval_end = if idx + 1 < sorted_intervals.len() {
            now - Duration::days(sorted_intervals[idx + 1].after_days as i64)
        } else {
            DateTime::<Utc>::MIN_UTC
        };

        // Get backups in this interval
        let backups_in_interval: Vec<&BackupMetadata> = sorted_backups
            .iter()
            .filter_map(|b| {
                let timestamp = b.end_time.unwrap_or(b.start_time);
                if timestamp <= interval_start && timestamp > interval_end {
                    Some(*b)
                } else {
                    None
                }
            })
            .collect();

        // Select backups spaced by spacing_hours or spacing_days
        let selected = select_spaced_backups(
            &backups_in_interval,
            interval.keep_count,
            interval.spacing_hours,
            interval.spacing_days,
        );

        for backup_id in selected {
            to_keep_ids.insert(backup_id);
        }
    }

    // Ensure minimum backup count
    if to_keep_ids.len() < minimum_backups {
        for backup in sorted_backups.iter().take(minimum_backups) {
            to_keep_ids.insert(backup.id.clone());
        }

        if to_keep_ids.len() < minimum_backups {
            warnings.push(format!(
                "Only {} completed backups available, need {} minimum",
                to_keep_ids.len(),
                minimum_backups
            ));
        }
    }

    // Always keep pinned backups
    for backup in backups.iter().filter(|b| b.pinned) {
        to_keep_ids.insert(backup.id.clone());
    }

    // Preserve chains if needed
    if preserve_chains {
        preserve_backup_chains(backups, &mut to_keep_ids);
    }

    // Apply minimum successful backups safety check
    let completed_count = to_keep_ids
        .iter()
        .filter(|id| {
            backups
                .iter()
                .find(|b| &b.id == *id)
                .map(|b| b.status == BackupStatus::Completed)
                .unwrap_or(false)
        })
        .count();

    if completed_count < policy.safety.min_successful_backups {
        warnings.push(format!(
            "Safety check: Would keep only {} successful backups, minimum is {}",
            completed_count, policy.safety.min_successful_backups
        ));
    }

    build_evaluation(backups, to_keep_ids, &warnings)
}

/// Selects backups spaced by a specific interval
fn select_spaced_backups(
    backups: &[&BackupMetadata],
    keep_count: usize,
    spacing_hours: Option<u32>,
    spacing_days: u32,
) -> Vec<String> {
    if backups.is_empty() {
        return Vec::new();
    }

    let mut selected = Vec::new();
    let mut last_selected: Option<DateTime<Utc>> = None;

    // Use hours if specified, otherwise use days
    let spacing = if let Some(hours) = spacing_hours {
        Duration::hours(hours as i64)
    } else {
        Duration::days(spacing_days as i64)
    };

    for backup in backups {
        let timestamp = backup.end_time.unwrap_or(backup.start_time);

        if let Some(last) = last_selected {
            // Check if enough time has passed since last selected backup
            if timestamp <= last - spacing {
                selected.push(backup.id.clone());
                last_selected = Some(timestamp);

                if selected.len() >= keep_count {
                    break;
                }
            }
        } else {
            // First backup in interval
            selected.push(backup.id.clone());
            last_selected = Some(timestamp);

            if selected.len() >= keep_count {
                break;
            }
        }
    }

    selected
}

/// Preserves backup chains by keeping all incrementals for kept full backups
fn preserve_backup_chains(backups: &[BackupMetadata], to_keep_ids: &mut HashSet<String>) {
    // First pass: propagate downward from kept full backups to their incrementals
    let kept_full_backups: HashSet<_> = to_keep_ids
        .iter()
        .filter(|id| {
            backups
                .iter()
                .find(|b| &b.id == *id)
                .map(|b| b.backup_type == BackupType::Full)
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    // Add all incrementals that depend on kept full backups
    for backup in backups {
        if backup.backup_type == BackupType::Incremental {
            if let Some(base_id) = &backup.base_backup_id {
                if kept_full_backups.contains(base_id) {
                    to_keep_ids.insert(backup.id.clone());
                }
            }
        }
    }

    // Second pass: propagate upward from kept incrementals to their base backups
    // Repeat until no new IDs are added to ensure full chain preservation
    loop {
        let initial_size = to_keep_ids.len();

        for backup in backups {
            // If this backup is kept and is an incremental, ensure its base is also kept
            if to_keep_ids.contains(&backup.id) && backup.backup_type == BackupType::Incremental {
                if let Some(base_id) = &backup.base_backup_id {
                    // Check if the base backup exists
                    if backups.iter().any(|b| b.id == *base_id) {
                        to_keep_ids.insert(base_id.clone());
                    }
                    // If base doesn't exist, we skip it (orphaned incremental)
                }
            }
        }

        // If no new IDs were added, we're done
        if to_keep_ids.len() == initial_size {
            break;
        }
    }
}

/// Builds the final purge evaluation
fn build_evaluation(
    backups: &[BackupMetadata],
    to_keep_ids: HashSet<String>,
    warnings: &[String],
) -> Result<PurgeEvaluation, StorageError> {
    let mut to_keep = Vec::new();
    let mut to_delete = Vec::new();
    let mut estimated_space_freed = 0u64;

    // Build lookup for dependent incrementals
    let mut has_dependents: HashMap<String, bool> = HashMap::new();
    for backup in backups {
        if backup.backup_type == BackupType::Incremental {
            if let Some(base_id) = &backup.base_backup_id {
                has_dependents.insert(base_id.clone(), true);
            }
        }
    }

    for backup in backups {
        // Check if this backup has dependents
        let has_deps = has_dependents.get(&backup.id).copied().unwrap_or(false);

        let decision = if to_keep_ids.contains(&backup.id) {
            create_keep_decision(
                backup,
                &determine_keep_reason(backup, &to_keep_ids),
                has_deps,
            )
        } else {
            estimated_space_freed += backup.size_bytes;
            create_delete_decision(backup, &determine_delete_reason(backup), has_deps)
        };

        if to_keep_ids.contains(&backup.id) {
            to_keep.push(decision);
        } else {
            to_delete.push(decision);
        }
    }

    // Sort by timestamp
    to_keep.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    to_delete.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Ok(PurgeEvaluation {
        timestamp: Utc::now(),
        total_backups: backups.len(),
        to_keep,
        to_delete,
        warnings: warnings.to_vec(),
        estimated_space_freed,
    })
}

fn create_keep_decision(
    backup: &BackupMetadata,
    reason: &str,
    has_dependents: bool,
) -> BackupPurgeDecision {
    BackupPurgeDecision {
        backup_id: backup.id.clone(),
        backup_type: backup.backup_type.clone(),
        timestamp: backup.end_time.unwrap_or(backup.start_time),
        size_bytes: backup.size_bytes,
        reason: reason.to_string(),
        pinned: backup.pinned,
        has_dependents,
    }
}

fn create_delete_decision(
    backup: &BackupMetadata,
    reason: &str,
    has_dependents: bool,
) -> BackupPurgeDecision {
    BackupPurgeDecision {
        backup_id: backup.id.clone(),
        backup_type: backup.backup_type.clone(),
        timestamp: backup.end_time.unwrap_or(backup.start_time),
        size_bytes: backup.size_bytes,
        reason: reason.to_string(),
        pinned: backup.pinned,
        has_dependents,
    }
}

fn determine_keep_reason(backup: &BackupMetadata, _to_keep_ids: &HashSet<String>) -> String {
    if backup.pinned {
        return "Pinned by user".to_string();
    }

    if backup.status != BackupStatus::Completed {
        return format!("Status: {:?}", backup.status);
    }

    match backup.backup_type {
        BackupType::Full => "Full backup within retention policy".to_string(),
        BackupType::Incremental => "Incremental backup within retention policy".to_string(),
        BackupType::Snapshot => "Snapshot within retention policy".to_string(),
    }
}

fn determine_delete_reason(backup: &BackupMetadata) -> String {
    if backup.status == BackupStatus::Failed {
        return "Failed backup".to_string();
    }

    "Outside retention policy window".to_string()
}

/// Reports purge operation to Sentry
pub fn report_purge_to_sentry(report: &PurgeReport, policy: &RetentionPolicy) {
    if !policy.notifications.sentry_enabled {
        return;
    }

    if report.failed > 0 && policy.notifications.report_errors {
        sentry::capture_message(
            &format!("Purge failed for {} backups", report.failed),
            sentry::Level::Error,
        );

        for error in &report.errors {
            sentry::capture_message(error, sentry::Level::Error);
        }
    }

    if policy.notifications.report_summary {
        sentry::add_breadcrumb(sentry::Breadcrumb {
            ty: "info".into(),
            category: Some("backup.purge".into()),
            message: Some(format!(
                "Purged {} backups, freed {} bytes, kept {}",
                report.deleted, report.space_freed, report.kept
            )),
            ..Default::default()
        });
    }
}
