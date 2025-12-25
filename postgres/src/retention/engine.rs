//! PITR-aware retention engine for backup and WAL retention decisions.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use super::policy::{IntervalSpec, PitrRetentionPolicy, RetentionRule};
use super::wal::{parse_lsn, WalInventory};
use super::{
    BackupItem, BackupItemStatus, BackupItemType, BackupLocation, RetentionResult,
    WalRetentionDecision,
};

/// Decision for a single backup in retention evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionDecision {
    /// Backup ID
    pub backup_id: String,
    /// Type of backup
    pub backup_type: BackupItemType,
    /// When the backup was created
    pub timestamp: DateTime<Utc>,
    /// Size in bytes
    pub size_bytes: u64,
    /// Reason for keeping or deleting
    pub reason: String,
    /// Whether the backup is pinned
    pub pinned: bool,
    /// Whether this backup has dependent incremental backups
    pub has_dependents: bool,
    /// Location of the backup
    pub location: BackupLocation,
}

/// Result of evaluating retention for a set of backups
#[derive(Debug, Clone)]
pub struct RetentionEvaluation {
    /// Backups to keep (ID -> reason)
    pub to_keep: HashMap<String, String>,
    /// Backups to delete (ID -> reason)
    pub to_delete: HashMap<String, String>,
    /// WAL segments to keep
    pub wal_to_keep: HashSet<String>,
    /// WAL segments to delete
    pub wal_to_delete: HashSet<String>,
    /// Warnings generated during evaluation
    pub warnings: Vec<String>,
}

impl RetentionEvaluation {
    #[allow(dead_code)] // Used internally by retention engine
    fn new() -> Self {
        Self {
            to_keep: HashMap::new(),
            to_delete: HashMap::new(),
            wal_to_keep: HashSet::new(),
            wal_to_delete: HashSet::new(),
            warnings: Vec::new(),
        }
    }
}

/// The retention engine evaluates backups and WAL segments against a policy
pub struct RetentionEngine {
    policy: PitrRetentionPolicy,
}

impl RetentionEngine {
    /// Creates a new retention engine with the given policy
    pub fn new(policy: PitrRetentionPolicy) -> Self {
        Self { policy }
    }

    /// Evaluates which backups and WAL segments to keep or delete
    pub fn evaluate(
        &self,
        backups: &[BackupItem],
        wal_inventory: Option<&WalInventory>,
    ) -> RetentionResult {
        let mut result = RetentionResult::new();
        result.total_backups = backups.len();
        result.total_wal_segments = wal_inventory.map(|w| w.segments.len()).unwrap_or(0);

        if !self.policy.enabled {
            result
                .warnings
                .push("Retention policy is disabled".to_string());
            // Keep everything when disabled
            for backup in backups {
                result.backups_to_keep.push(self.create_keep_decision(
                    backup,
                    "Policy is disabled",
                    false,
                ));
            }
            return result;
        }

        // Filter backups by scope
        let in_scope_backups: Vec<&BackupItem> =
            backups.iter().filter(|b| self.is_in_scope(b)).collect();

        // Evaluate backup retention
        let mut keep_ids: HashSet<String> = HashSet::new();
        let mut keep_reasons: HashMap<String, String> = HashMap::new();

        // Apply each rule in order
        for rule in &self.policy.rules {
            self.apply_rule(rule, &in_scope_backups, &mut keep_ids, &mut keep_reasons);
        }

        // Apply safety checks
        self.apply_safety_checks(
            &in_scope_backups,
            &mut keep_ids,
            &mut keep_reasons,
            &mut result.warnings,
        );

        // Preserve backup chains if configured
        if self.policy.safety.preserve_chains {
            self.preserve_chains(&in_scope_backups, &mut keep_ids, &mut keep_reasons);
        }

        // Build backup decisions
        let dependents = self.find_dependents(&in_scope_backups);
        for backup in &in_scope_backups {
            let has_deps = dependents.contains(&backup.id);
            if keep_ids.contains(&backup.id) {
                let reason = keep_reasons
                    .get(&backup.id)
                    .cloned()
                    .unwrap_or_else(|| "Retained by policy".to_string());
                result
                    .backups_to_keep
                    .push(self.create_keep_decision(backup, &reason, has_deps));
            } else {
                let reason = self.determine_delete_reason(backup);
                result
                    .backups_to_delete
                    .push(self.create_delete_decision(backup, &reason, has_deps));
            }
        }

        // Handle out-of-scope backups (keep them with a note)
        for backup in backups.iter().filter(|b| !self.is_in_scope(b)) {
            result.backups_to_keep.push(self.create_keep_decision(
                backup,
                "Out of policy scope",
                false,
            ));
        }

        // Evaluate WAL retention if inventory provided
        if let Some(wal_inv) = wal_inventory {
            self.evaluate_wal_retention(wal_inv, &keep_ids, backups, &mut result);
        }

        // Calculate PITR window
        if let Some(wal_inv) = wal_inventory {
            if let Some((start, end)) = wal_inv.pitr_window() {
                result.pitr_window_start = Some(start);
                result.pitr_window_end = Some(end);
            }
        }

        // Sort results by timestamp
        result.backups_to_keep.sort_by_key(|d| Reverse(d.timestamp));
        result
            .backups_to_delete
            .sort_by_key(|d| Reverse(d.timestamp));

        result.calculate_space_freed();
        result
    }

    /// Checks if a backup is within the policy scope
    fn is_in_scope(&self, backup: &BackupItem) -> bool {
        let scope = &self.policy.scope;

        // Check database filter
        if !scope.databases.is_empty() {
            if let Some(db) = &backup.database {
                if !scope.databases.contains(db) {
                    return false;
                }
            }
        }

        // Check backup type filter
        if !scope.backup_types.is_empty() {
            let type_str = match backup.backup_type {
                BackupItemType::Full => "full",
                BackupItemType::Incremental => "incremental",
                BackupItemType::Snapshot => "snapshot",
            };
            if !scope.backup_types.iter().any(|t| t == type_str) {
                return false;
            }
        }

        // Check excluded tags
        for tag in &scope.exclude_tags {
            if backup.tags.contains(tag) {
                return false;
            }
        }

        true
    }

    /// Applies a single retention rule
    fn apply_rule(
        &self,
        rule: &RetentionRule,
        backups: &[&BackupItem],
        keep_ids: &mut HashSet<String>,
        keep_reasons: &mut HashMap<String, String>,
    ) {
        match rule {
            RetentionRule::KeepPinned => {
                for backup in backups.iter().filter(|b| b.pinned) {
                    keep_ids.insert(backup.id.clone());
                    keep_reasons.insert(backup.id.clone(), "Pinned by user".to_string());
                }
            }

            RetentionRule::KeepLatest { count } => {
                let mut sorted: Vec<_> = backups.iter().filter(|b| b.is_completed()).collect();
                sorted.sort_by_key(|b| Reverse(b.effective_time()));

                for backup in sorted.iter().take(*count) {
                    keep_ids.insert(backup.id.clone());
                    keep_reasons
                        .insert(backup.id.clone(), format!("Keep latest {} backups", count));
                }
            }

            RetentionRule::KeepWithinDays { days, minimum } => {
                let cutoff = Utc::now() - Duration::days(*days as i64);
                let mut kept_count = 0;

                // First, keep all within time window
                for backup in backups.iter().filter(|b| b.is_completed()) {
                    if backup.effective_time() >= cutoff {
                        keep_ids.insert(backup.id.clone());
                        keep_reasons.insert(
                            backup.id.clone(),
                            format!("Within {} day retention window", days),
                        );
                        kept_count += 1;
                    }
                }

                // Then ensure minimum count
                if kept_count < *minimum {
                    let mut sorted: Vec<_> = backups
                        .iter()
                        .filter(|b| b.is_completed() && !keep_ids.contains(&b.id))
                        .collect();
                    sorted.sort_by_key(|b| Reverse(b.effective_time()));

                    for backup in sorted.iter().take(*minimum - kept_count) {
                        keep_ids.insert(backup.id.clone());
                        keep_reasons.insert(
                            backup.id.clone(),
                            format!("Minimum {} backups required", minimum),
                        );
                    }
                }
            }

            RetentionRule::KeepIntervals {
                hourly,
                daily,
                weekly,
                monthly,
                yearly,
            } => {
                let now = Utc::now();

                if let Some(spec) = hourly {
                    self.apply_interval_rule(
                        backups,
                        keep_ids,
                        keep_reasons,
                        spec,
                        1,
                        "hourly",
                        now,
                    );
                }
                if let Some(spec) = daily {
                    self.apply_interval_rule(
                        backups,
                        keep_ids,
                        keep_reasons,
                        spec,
                        24,
                        "daily",
                        now,
                    );
                }
                if let Some(spec) = weekly {
                    self.apply_interval_rule(
                        backups,
                        keep_ids,
                        keep_reasons,
                        spec,
                        24 * 7,
                        "weekly",
                        now,
                    );
                }
                if let Some(spec) = monthly {
                    self.apply_interval_rule(
                        backups,
                        keep_ids,
                        keep_reasons,
                        spec,
                        24 * 30,
                        "monthly",
                        now,
                    );
                }
                if let Some(spec) = yearly {
                    self.apply_interval_rule(
                        backups,
                        keep_ids,
                        keep_reasons,
                        spec,
                        24 * 365,
                        "yearly",
                        now,
                    );
                }
            }

            RetentionRule::KeepTagged { tags } => {
                for backup in backups {
                    for tag in tags {
                        if backup.tags.contains(tag) {
                            keep_ids.insert(backup.id.clone());
                            keep_reasons
                                .insert(backup.id.clone(), format!("Tagged with '{}'", tag));
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Applies an interval-based retention rule (GFS-style)
    #[allow(clippy::too_many_arguments)]
    fn apply_interval_rule(
        &self,
        backups: &[&BackupItem],
        keep_ids: &mut HashSet<String>,
        keep_reasons: &mut HashMap<String, String>,
        spec: &IntervalSpec,
        interval_hours: i64,
        interval_name: &str,
        now: DateTime<Utc>,
    ) {
        let max_age = spec
            .max_age_days
            .map(|d| Duration::days(d as i64))
            .unwrap_or(Duration::days(36500)); // ~100 years

        let cutoff = now - max_age;
        let interval = Duration::hours(interval_hours);

        // Get completed backups within age limit, sorted newest first
        let mut candidates: Vec<_> = backups
            .iter()
            .filter(|b| b.is_completed() && b.effective_time() >= cutoff)
            .collect();
        candidates.sort_by_key(|b| Reverse(b.effective_time()));

        // Select backups spaced by the interval
        let mut selected = Vec::new();
        let mut last_selected: Option<DateTime<Utc>> = None;

        for backup in candidates {
            let timestamp = backup.effective_time();

            if let Some(last) = last_selected {
                // Check if enough time has passed since last selected
                if last - timestamp >= interval {
                    selected.push(backup);
                    last_selected = Some(timestamp);
                }
            } else {
                // First backup
                selected.push(backup);
                last_selected = Some(timestamp);
            }

            if selected.len() >= spec.count {
                break;
            }
        }

        for backup in selected {
            keep_ids.insert(backup.id.clone());
            keep_reasons.insert(
                backup.id.clone(),
                format!("Keep {} {} backups", spec.count, interval_name),
            );
        }
    }

    /// Applies safety checks to ensure minimum requirements are met
    fn apply_safety_checks(
        &self,
        backups: &[&BackupItem],
        keep_ids: &mut HashSet<String>,
        keep_reasons: &mut HashMap<String, String>,
        warnings: &mut Vec<String>,
    ) {
        let safety = &self.policy.safety;

        // Ensure minimum successful backups
        let successful_kept: Vec<_> = backups
            .iter()
            .filter(|b| b.is_completed() && keep_ids.contains(&b.id))
            .collect();

        if successful_kept.len() < safety.min_successful_backups {
            let needed = safety.min_successful_backups - successful_kept.len();
            let mut additional: Vec<_> = backups
                .iter()
                .filter(|b| b.is_completed() && !keep_ids.contains(&b.id))
                .collect();
            additional.sort_by_key(|b| Reverse(b.effective_time()));

            for backup in additional.iter().take(needed) {
                keep_ids.insert(backup.id.clone());
                keep_reasons.insert(
                    backup.id.clone(),
                    format!(
                        "Safety: minimum {} successful backups",
                        safety.min_successful_backups
                    ),
                );
            }

            if additional.len() < needed {
                warnings.push(format!(
                    "Only {} successful backups available, need {} minimum",
                    successful_kept.len() + additional.len(),
                    safety.min_successful_backups
                ));
            }
        }

        // Keep latest successful backup
        if safety.keep_latest_successful {
            if let Some(latest) = backups
                .iter()
                .filter(|b| b.is_completed())
                .max_by_key(|b| b.effective_time())
            {
                keep_ids.insert(latest.id.clone());
                keep_reasons.insert(
                    latest.id.clone(),
                    "Safety: keep latest successful backup".to_string(),
                );
            }
        }
    }

    /// Preserves backup chains by keeping base backups for retained incrementals
    fn preserve_chains(
        &self,
        backups: &[&BackupItem],
        keep_ids: &mut HashSet<String>,
        keep_reasons: &mut HashMap<String, String>,
    ) {
        // Build a map of backup IDs to backups
        let backup_map: HashMap<_, _> = backups.iter().map(|b| (b.id.clone(), *b)).collect();

        // Find all base backups needed for kept incrementals
        let mut bases_needed: HashSet<String> = HashSet::new();

        for backup in backups {
            if keep_ids.contains(&backup.id) && backup.is_incremental() {
                if let Some(base_id) = &backup.base_backup_id {
                    bases_needed.insert(base_id.clone());
                }
            }
        }

        // Recursively find all ancestors
        let mut to_check: Vec<String> = bases_needed.iter().cloned().collect();
        while let Some(id) = to_check.pop() {
            if let Some(backup) = backup_map.get(&id) {
                if !keep_ids.contains(&id) {
                    keep_ids.insert(id.clone());
                    keep_reasons.insert(
                        id.clone(),
                        "Chain preservation: base for retained incremental".to_string(),
                    );
                }
                if let Some(base_id) = &backup.base_backup_id {
                    if !keep_ids.contains(base_id) {
                        to_check.push(base_id.clone());
                    }
                }
            }
        }
    }

    /// Finds backups that have dependents (incrementals pointing to them)
    fn find_dependents(&self, backups: &[&BackupItem]) -> HashSet<String> {
        let mut dependents = HashSet::new();
        for backup in backups {
            if let Some(base_id) = &backup.base_backup_id {
                dependents.insert(base_id.clone());
            }
        }
        dependents
    }

    /// Evaluates WAL segment retention
    fn evaluate_wal_retention(
        &self,
        wal_inventory: &WalInventory,
        kept_backup_ids: &HashSet<String>,
        backups: &[BackupItem],
        result: &mut RetentionResult,
    ) {
        let wal_config = &self.policy.wal_retention;
        let now = Utc::now();
        let pitr_cutoff = now - wal_config.pitr_window();

        // Find the oldest LSN we need to keep for PITR
        let mut min_lsn_needed: Option<u64> = None;

        // If keeping WAL for retained backups, find the oldest retained backup's LSN
        if wal_config.keep_for_retained_backups {
            for backup in backups {
                if kept_backup_ids.contains(&backup.id) {
                    if let Some(wal_start) = &backup.wal_start {
                        if let Some(lsn) = parse_lsn(wal_start) {
                            min_lsn_needed = Some(min_lsn_needed.map_or(lsn, |m| m.min(lsn)));
                        }
                    }
                }
            }
        }

        // Evaluate each WAL segment
        for segment in &wal_inventory.segments {
            let mut keep = false;
            let mut reason = String::new();

            // Check PITR window
            if let Some(modified) = segment.last_modified {
                if modified >= pitr_cutoff {
                    keep = true;
                    reason = format!(
                        "Within PITR window ({} hours)",
                        wal_config.pitr_window_hours
                    );
                }
            }

            // Check if needed for retained backups
            if !keep {
                if let Some(min_lsn) = min_lsn_needed {
                    if segment.lsn() >= min_lsn {
                        keep = true;
                        reason = "Required for retained backup recovery".to_string();
                    }
                }
            }

            // Check max age
            if keep {
                if let Some(max_age) = wal_config.max_age() {
                    if let Some(modified) = segment.last_modified {
                        if now - modified > max_age {
                            keep = false;
                            reason = format!(
                                "Exceeds max WAL age ({} days)",
                                wal_config.max_wal_age_days.unwrap_or(0)
                            );
                        }
                    }
                }
            }

            // Always keep metadata files
            if segment.is_metadata {
                keep = true;
                reason = "WAL metadata file".to_string();
            }

            let decision = WalRetentionDecision {
                segment_name: segment.name.clone(),
                size_bytes: segment.size_bytes,
                reason: reason.clone(),
                location: segment.location.clone(),
                timeline: segment.timeline,
            };

            if keep {
                result.wal_to_keep.push(decision);
            } else if reason.is_empty() {
                let mut decision = decision;
                decision.reason = "Outside retention window".to_string();
                result.wal_to_delete.push(decision);
            } else {
                result.wal_to_delete.push(decision);
            }
        }

        // Check PITR window safety
        if let Some(min_hours) = self.policy.safety.min_pitr_window_hours {
            let min_window = Duration::hours(min_hours as i64);
            if let Some((start, end)) = wal_inventory.pitr_window() {
                let actual_window = end - start;
                if actual_window < min_window {
                    result.warnings.push(format!(
                        "PITR window ({} hours) is below minimum ({} hours)",
                        actual_window.num_hours(),
                        min_hours
                    ));
                }
            }
        }
    }

    /// Creates a keep decision for a backup
    fn create_keep_decision(
        &self,
        backup: &BackupItem,
        reason: &str,
        has_dependents: bool,
    ) -> RetentionDecision {
        RetentionDecision {
            backup_id: backup.id.clone(),
            backup_type: backup.backup_type,
            timestamp: backup.effective_time(),
            size_bytes: backup.size_bytes,
            reason: reason.to_string(),
            pinned: backup.pinned,
            has_dependents,
            location: backup.location.clone(),
        }
    }

    /// Creates a delete decision for a backup
    fn create_delete_decision(
        &self,
        backup: &BackupItem,
        reason: &str,
        has_dependents: bool,
    ) -> RetentionDecision {
        RetentionDecision {
            backup_id: backup.id.clone(),
            backup_type: backup.backup_type,
            timestamp: backup.effective_time(),
            size_bytes: backup.size_bytes,
            reason: reason.to_string(),
            pinned: backup.pinned,
            has_dependents,
            location: backup.location.clone(),
        }
    }

    /// Determines the reason for deleting a backup
    fn determine_delete_reason(&self, backup: &BackupItem) -> String {
        if backup.status == BackupItemStatus::Failed {
            return "Failed backup".to_string();
        }
        if backup.status == BackupItemStatus::InProgress {
            return "Incomplete backup".to_string();
        }
        "Outside retention policy".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::policy::SafetySettings;
    use super::*;

    fn create_test_backup(id: &str, days_ago: i64, backup_type: BackupItemType) -> BackupItem {
        let now = Utc::now();
        BackupItem {
            id: id.to_string(),
            backup_type,
            status: BackupItemStatus::Completed,
            start_time: now - Duration::days(days_ago) - Duration::hours(1),
            end_time: Some(now - Duration::days(days_ago)),
            base_backup_id: None,
            wal_start: None,
            wal_end: None,
            size_bytes: 1024 * 1024 * 100, // 100 MB
            database: Some("testdb".to_string()),
            pinned: false,
            tags: vec![],
            location: BackupLocation::Local(format!("/backups/{}", id)),
        }
    }

    #[test]
    fn test_keep_latest_rule() {
        let policy = PitrRetentionPolicy {
            rules: vec![RetentionRule::KeepLatest { count: 2 }],
            ..Default::default()
        };

        let engine = RetentionEngine::new(policy);

        let backups = vec![
            create_test_backup("backup1", 3, BackupItemType::Full),
            create_test_backup("backup2", 2, BackupItemType::Full),
            create_test_backup("backup3", 1, BackupItemType::Full),
        ];

        let result = engine.evaluate(&backups, None);

        assert_eq!(result.backups_to_keep.len(), 2);
        assert_eq!(result.backups_to_delete.len(), 1);
        assert!(result
            .backups_to_keep
            .iter()
            .any(|d| d.backup_id == "backup3"));
        assert!(result
            .backups_to_keep
            .iter()
            .any(|d| d.backup_id == "backup2"));
        assert!(result
            .backups_to_delete
            .iter()
            .any(|d| d.backup_id == "backup1"));
    }

    #[test]
    fn test_keep_within_days_rule() {
        let policy = PitrRetentionPolicy {
            rules: vec![RetentionRule::KeepWithinDays {
                days: 5,
                minimum: 1,
            }],
            ..Default::default()
        };

        let engine = RetentionEngine::new(policy);

        let backups = vec![
            create_test_backup("old", 10, BackupItemType::Full),
            create_test_backup("recent", 2, BackupItemType::Full),
        ];

        let result = engine.evaluate(&backups, None);

        assert!(result
            .backups_to_keep
            .iter()
            .any(|d| d.backup_id == "recent"));
        assert!(result
            .backups_to_delete
            .iter()
            .any(|d| d.backup_id == "old"));
    }

    #[test]
    fn test_pinned_backups_always_kept() {
        let policy = PitrRetentionPolicy {
            rules: vec![
                RetentionRule::KeepPinned,
                RetentionRule::KeepLatest { count: 1 },
            ],
            ..Default::default()
        };

        let engine = RetentionEngine::new(policy);

        let mut old_backup = create_test_backup("old_pinned", 30, BackupItemType::Full);
        old_backup.pinned = true;

        let backups = vec![
            old_backup,
            create_test_backup("recent", 1, BackupItemType::Full),
        ];

        let result = engine.evaluate(&backups, None);

        assert_eq!(result.backups_to_keep.len(), 2);
        assert!(result
            .backups_to_keep
            .iter()
            .any(|d| d.backup_id == "old_pinned"));
    }

    #[test]
    fn test_chain_preservation() {
        let policy = PitrRetentionPolicy {
            rules: vec![RetentionRule::KeepLatest { count: 1 }],
            safety: SafetySettings {
                preserve_chains: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let engine = RetentionEngine::new(policy);

        let full_backup = create_test_backup("full", 5, BackupItemType::Full);
        let mut incr_backup = create_test_backup("incr", 1, BackupItemType::Incremental);
        incr_backup.base_backup_id = Some("full".to_string());

        let backups = vec![full_backup, incr_backup];

        let result = engine.evaluate(&backups, None);

        // Both should be kept because incr depends on full
        assert_eq!(result.backups_to_keep.len(), 2);
    }

    #[test]
    fn test_minimum_successful_backups() {
        let policy = PitrRetentionPolicy {
            rules: vec![RetentionRule::KeepLatest { count: 1 }],
            safety: SafetySettings {
                min_successful_backups: 3,
                ..Default::default()
            },
            ..Default::default()
        };

        let engine = RetentionEngine::new(policy);

        let backups = vec![
            create_test_backup("backup1", 5, BackupItemType::Full),
            create_test_backup("backup2", 3, BackupItemType::Full),
            create_test_backup("backup3", 1, BackupItemType::Full),
        ];

        let result = engine.evaluate(&backups, None);

        // All 3 should be kept due to minimum requirement
        assert_eq!(result.backups_to_keep.len(), 3);
    }

    #[test]
    fn test_disabled_policy_keeps_all() {
        let policy = PitrRetentionPolicy {
            enabled: false,
            ..Default::default()
        };

        let engine = RetentionEngine::new(policy);

        let backups = vec![
            create_test_backup("backup1", 100, BackupItemType::Full),
            create_test_backup("backup2", 50, BackupItemType::Full),
        ];

        let result = engine.evaluate(&backups, None);

        assert_eq!(result.backups_to_keep.len(), 2);
        assert_eq!(result.backups_to_delete.len(), 0);
        assert!(result.warnings.iter().any(|w| w.contains("disabled")));
    }
}
