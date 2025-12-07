//! Unit tests for backup retention policy evaluation.
//!
//! These tests verify that the retention engine correctly evaluates
//! various backup scenarios against different retention policies.

use chrono::{Duration, Utc};
use postgres::retention::{
    BackupItem, BackupItemStatus, BackupItemType, BackupLocation, PitrRetentionPolicy,
    RetentionEngine, RetentionRule, SafetySettings, WalRetentionConfig,
};

/// Helper to create a test backup
fn create_backup(
    id: &str,
    days_ago: i64,
    backup_type: BackupItemType,
    status: BackupItemStatus,
) -> BackupItem {
    let now = Utc::now();
    BackupItem {
        id: id.to_string(),
        backup_type,
        status,
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

/// Helper to create a completed full backup
fn full_backup(id: &str, days_ago: i64) -> BackupItem {
    create_backup(id, days_ago, BackupItemType::Full, BackupItemStatus::Completed)
}

/// Helper to create a completed incremental backup
fn incremental_backup(id: &str, days_ago: i64, base_id: &str) -> BackupItem {
    let mut backup = create_backup(
        id,
        days_ago,
        BackupItemType::Incremental,
        BackupItemStatus::Completed,
    );
    backup.base_backup_id = Some(base_id.to_string());
    backup
}

/// Helper to create a pinned backup
fn pinned_backup(id: &str, days_ago: i64) -> BackupItem {
    let mut backup = full_backup(id, days_ago);
    backup.pinned = true;
    backup
}

/// Helper to create a failed backup
fn failed_backup(id: &str, days_ago: i64) -> BackupItem {
    create_backup(id, days_ago, BackupItemType::Full, BackupItemStatus::Failed)
}

// ============================================================================
// Basic Retention Rules Tests
// ============================================================================

#[test]
fn test_keep_latest_n_backups() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 3 }],
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("backup1", 5),
        full_backup("backup2", 4),
        full_backup("backup3", 3),
        full_backup("backup4", 2),
        full_backup("backup5", 1),
    ];

    let result = engine.evaluate(&backups, None);

    // Should keep the 3 most recent
    assert_eq!(result.backups_to_keep.len(), 3);
    assert_eq!(result.backups_to_delete.len(), 2);

    let kept_ids: Vec<_> = result.backups_to_keep.iter().map(|d| &d.backup_id).collect();
    assert!(kept_ids.contains(&&"backup5".to_string()));
    assert!(kept_ids.contains(&&"backup4".to_string()));
    assert!(kept_ids.contains(&&"backup3".to_string()));

    let deleted_ids: Vec<_> = result.backups_to_delete.iter().map(|d| &d.backup_id).collect();
    assert!(deleted_ids.contains(&&"backup1".to_string()));
    assert!(deleted_ids.contains(&&"backup2".to_string()));
}

#[test]
fn test_keep_within_days() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepWithinDays {
            days: 7,
            minimum: 1,
        }],
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("old1", 30),
        full_backup("old2", 15),
        full_backup("recent1", 5),
        full_backup("recent2", 2),
    ];

    let result = engine.evaluate(&backups, None);

    // Should keep backups within 7 days
    assert_eq!(result.backups_to_keep.len(), 2);
    assert_eq!(result.backups_to_delete.len(), 2);

    let kept_ids: Vec<_> = result.backups_to_keep.iter().map(|d| &d.backup_id).collect();
    assert!(kept_ids.contains(&&"recent1".to_string()));
    assert!(kept_ids.contains(&&"recent2".to_string()));
}

#[test]
fn test_keep_within_days_minimum() {
    // When no backups are within the window, keep minimum
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepWithinDays {
            days: 7,
            minimum: 2,
        }],
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("old1", 30),
        full_backup("old2", 20),
        full_backup("old3", 15),
    ];

    let result = engine.evaluate(&backups, None);

    // Should keep minimum 2 even though all are outside window
    assert_eq!(result.backups_to_keep.len(), 2);
    assert_eq!(result.backups_to_delete.len(), 1);

    // Should keep the most recent ones
    let kept_ids: Vec<_> = result.backups_to_keep.iter().map(|d| &d.backup_id).collect();
    assert!(kept_ids.contains(&&"old3".to_string()));
    assert!(kept_ids.contains(&&"old2".to_string()));
}

#[test]
fn test_pinned_backups_always_kept() {
    let policy = PitrRetentionPolicy {
        rules: vec![
            RetentionRule::KeepPinned,
            RetentionRule::KeepLatest { count: 1 },
        ],
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        pinned_backup("pinned_old", 100),
        full_backup("recent", 1),
    ];

    let result = engine.evaluate(&backups, None);

    // Both should be kept - pinned and latest
    assert_eq!(result.backups_to_keep.len(), 2);
    assert_eq!(result.backups_to_delete.len(), 0);

    let pinned_decision = result
        .backups_to_keep
        .iter()
        .find(|d| d.backup_id == "pinned_old")
        .unwrap();
    assert!(pinned_decision.pinned);
    assert!(pinned_decision.reason.contains("Pinned"));
}

#[test]
fn test_tagged_backups_kept() {
    let policy = PitrRetentionPolicy {
        rules: vec![
            RetentionRule::KeepTagged {
                tags: vec!["important".to_string(), "compliance".to_string()],
            },
            RetentionRule::KeepLatest { count: 1 },
        ],
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let mut tagged_backup = full_backup("tagged", 50);
    tagged_backup.tags = vec!["important".to_string()];

    let backups = vec![
        tagged_backup,
        full_backup("untagged", 30),
        full_backup("recent", 1),
    ];

    let result = engine.evaluate(&backups, None);

    // Tagged and recent should be kept
    assert_eq!(result.backups_to_keep.len(), 2);
    assert_eq!(result.backups_to_delete.len(), 1);

    let kept_ids: Vec<_> = result.backups_to_keep.iter().map(|d| &d.backup_id).collect();
    assert!(kept_ids.contains(&&"tagged".to_string()));
    assert!(kept_ids.contains(&&"recent".to_string()));
}

// ============================================================================
// Chain Preservation Tests
// ============================================================================

#[test]
fn test_chain_preservation_keeps_base() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 1 }],
        safety: SafetySettings {
            preserve_chains: true,
            min_successful_backups: 1,
            keep_latest_successful: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("full1", 10),
        incremental_backup("incr1", 1, "full1"),
    ];

    let result = engine.evaluate(&backups, None);

    // Both should be kept - incr1 is latest, full1 is its base
    assert_eq!(result.backups_to_keep.len(), 2);
    assert_eq!(result.backups_to_delete.len(), 0);

    let full_decision = result
        .backups_to_keep
        .iter()
        .find(|d| d.backup_id == "full1")
        .unwrap();
    assert!(full_decision.reason.contains("Chain preservation"));
}

#[test]
fn test_chain_preservation_multi_level() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 1 }],
        safety: SafetySettings {
            preserve_chains: true,
            min_successful_backups: 1,
            keep_latest_successful: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    // Chain: full1 <- incr1 <- incr2 (latest)
    let mut incr2 = incremental_backup("incr2", 1, "incr1");
    incr2.base_backup_id = Some("incr1".to_string());

    let backups = vec![
        full_backup("full1", 30),
        incremental_backup("incr1", 15, "full1"),
        incr2,
    ];

    let result = engine.evaluate(&backups, None);

    // All should be kept due to chain
    assert_eq!(result.backups_to_keep.len(), 3);
    assert_eq!(result.backups_to_delete.len(), 0);
}

#[test]
fn test_no_chain_preservation_orphans_incrementals() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 1 }],
        safety: SafetySettings {
            preserve_chains: false,
            min_successful_backups: 1,
            keep_latest_successful: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("full1", 10),
        incremental_backup("incr1", 1, "full1"),
    ];

    let result = engine.evaluate(&backups, None);

    // Only incr1 kept (latest), full1 deleted (orphaning incr1)
    assert_eq!(result.backups_to_keep.len(), 1);
    assert_eq!(result.backups_to_delete.len(), 1);
    assert_eq!(result.backups_to_keep[0].backup_id, "incr1");
}

// ============================================================================
// Safety Checks Tests
// ============================================================================

#[test]
fn test_minimum_successful_backups() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 1 }],
        safety: SafetySettings {
            min_successful_backups: 3,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("backup1", 5),
        full_backup("backup2", 3),
        full_backup("backup3", 1),
    ];

    let result = engine.evaluate(&backups, None);

    // All 3 should be kept due to minimum requirement
    assert_eq!(result.backups_to_keep.len(), 3);
    assert_eq!(result.backups_to_delete.len(), 0);
}

#[test]
fn test_keep_latest_successful() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepWithinDays {
            days: 1,
            minimum: 0,
        }],
        safety: SafetySettings {
            keep_latest_successful: true,
            min_successful_backups: 1,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("old", 30),
        full_backup("older", 60),
    ];

    let result = engine.evaluate(&backups, None);

    // At least one should be kept (latest successful)
    assert!(result.backups_to_keep.len() >= 1);

    let kept_ids: Vec<_> = result.backups_to_keep.iter().map(|d| &d.backup_id).collect();
    assert!(kept_ids.contains(&&"old".to_string()));
}

#[test]
fn test_failed_backups_not_counted() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 2 }],
        safety: SafetySettings {
            min_successful_backups: 2,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("success1", 5),
        failed_backup("failed1", 3),
        full_backup("success2", 1),
    ];

    let result = engine.evaluate(&backups, None);

    // Both successful backups should be kept
    let kept_successful: Vec<_> = result
        .backups_to_keep
        .iter()
        .filter(|d| d.backup_id != "failed1")
        .collect();
    assert_eq!(kept_successful.len(), 2);
}

// ============================================================================
// Disabled Policy Tests
// ============================================================================

#[test]
fn test_disabled_policy_keeps_all() {
    let policy = PitrRetentionPolicy {
        enabled: false,
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let backups = vec![
        full_backup("backup1", 100),
        full_backup("backup2", 50),
        full_backup("backup3", 1),
    ];

    let result = engine.evaluate(&backups, None);

    assert_eq!(result.backups_to_keep.len(), 3);
    assert_eq!(result.backups_to_delete.len(), 0);
    assert!(result.warnings.iter().any(|w| w.contains("disabled")));
}

// ============================================================================
// GFS (Grandfather-Father-Son) Policy Tests
// ============================================================================

#[test]
fn test_gfs_policy_intervals() {
    use postgres::retention::policy::IntervalSpec;

    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepIntervals {
            hourly: None,
            daily: Some(IntervalSpec {
                count: 7,
                max_age_days: Some(7),
            }),
            weekly: Some(IntervalSpec {
                count: 4,
                max_age_days: Some(30),
            }),
            monthly: None,
            yearly: None,
        }],
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    // Create backups for the last 30 days
    let backups: Vec<_> = (0..30)
        .map(|i| full_backup(&format!("backup_{}", i), i))
        .collect();

    let result = engine.evaluate(&backups, None);

    // Should keep some daily and some weekly
    assert!(result.backups_to_keep.len() > 0);
    assert!(result.backups_to_keep.len() < 30);
}

// ============================================================================
// Policy Validation Tests
// ============================================================================

#[test]
fn test_default_policy_valid() {
    let policy = PitrRetentionPolicy::default();
    assert!(policy.validate().is_ok());
}

#[test]
fn test_aggressive_policy_valid() {
    let policy = PitrRetentionPolicy::aggressive();
    assert!(policy.validate().is_ok());
}

#[test]
fn test_conservative_policy_valid() {
    let policy = PitrRetentionPolicy::conservative();
    assert!(policy.validate().is_ok());
}

#[test]
fn test_gfs_standard_policy_valid() {
    let policy = PitrRetentionPolicy::gfs_standard();
    assert!(policy.validate().is_ok());
}

#[test]
fn test_empty_rules_invalid() {
    let policy = PitrRetentionPolicy {
        rules: vec![],
        ..Default::default()
    };
    let result = policy.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err()[0].contains("at least one retention rule"));
}

#[test]
fn test_zero_min_backups_invalid() {
    let policy = PitrRetentionPolicy {
        safety: SafetySettings {
            min_successful_backups: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let result = policy.validate();
    assert!(result.is_err());
}

// ============================================================================
// Space Calculation Tests
// ============================================================================

#[test]
fn test_space_freed_calculation() {
    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 1 }],
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let mut backup1 = full_backup("backup1", 10);
    backup1.size_bytes = 1024 * 1024 * 500; // 500 MB

    let mut backup2 = full_backup("backup2", 1);
    backup2.size_bytes = 1024 * 1024 * 200; // 200 MB

    let backups = vec![backup1, backup2];

    let result = engine.evaluate(&backups, None);

    // backup1 should be deleted, freeing 500 MB
    assert_eq!(result.estimated_space_freed, 1024 * 1024 * 500);
}

// ============================================================================
// Scope Filtering Tests
// ============================================================================

#[test]
fn test_database_scope_filter() {
    use postgres::retention::RetentionScope;

    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 1 }],
        scope: RetentionScope {
            databases: vec!["prod".to_string()],
            ..Default::default()
        },
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let mut prod_backup = full_backup("prod_backup", 10);
    prod_backup.database = Some("prod".to_string());

    let mut dev_backup = full_backup("dev_backup", 5);
    dev_backup.database = Some("dev".to_string());

    let backups = vec![prod_backup, dev_backup];

    let result = engine.evaluate(&backups, None);

    // dev_backup should be kept (out of scope)
    // prod_backup should be evaluated by policy
    let kept_ids: Vec<_> = result.backups_to_keep.iter().map(|d| &d.backup_id).collect();
    assert!(kept_ids.contains(&&"dev_backup".to_string())); // Out of scope, kept
}

#[test]
fn test_exclude_tags_filter() {
    use postgres::retention::RetentionScope;

    let policy = PitrRetentionPolicy {
        rules: vec![RetentionRule::KeepLatest { count: 1 }],
        scope: RetentionScope {
            exclude_tags: vec!["temporary".to_string()],
            ..Default::default()
        },
        safety: SafetySettings {
            min_successful_backups: 1,
            keep_latest_successful: false,
            preserve_chains: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let engine = RetentionEngine::new(policy);

    let mut temp_backup = full_backup("temp_backup", 5);
    temp_backup.tags = vec!["temporary".to_string()];

    let backups = vec![
        temp_backup,
        full_backup("normal_backup", 1),
    ];

    let result = engine.evaluate(&backups, None);

    // temp_backup should be kept (excluded from policy scope)
    let kept_ids: Vec<_> = result.backups_to_keep.iter().map(|d| &d.backup_id).collect();
    assert!(kept_ids.contains(&&"temp_backup".to_string()));
}
