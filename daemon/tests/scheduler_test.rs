//! Integration tests for the scheduler module.

use chrono::{Duration, Utc};
use common::config::{C2AuthConfig, FeaturesConfig, WardenConfig};
use common::schedule::{
    BackupSchedule, BackupTarget, BackupType, ParsedSchedule, RetentionSchedule, ScheduleConfig,
    StorageProfile,
};
use daemon::scheduler::{Scheduler, SchedulerOptions};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn create_test_config_with_schedules() -> WardenConfig {
    WardenConfig {
        c2_server: "http://localhost:8080".to_string(),
        c2_auth: C2AuthConfig {
            id: "test".to_string(),
            secret: "test".to_string(),
        },
        features: FeaturesConfig {
            overwatch: false,
            postgres_backup: true,
        },
        mqtt: None,
        integration: common::config::IntegrationConfig::default(),
        schedules: Some(ScheduleConfig {
            backups: vec![
                BackupSchedule {
                    id: "every-minute".to_string(),
                    name: Some("Every Minute Backup".to_string()),
                    cron: "* * * * *".to_string(), // Every minute
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
                    backup_dir: Some("/tmp/test-backups".to_string()),
                    encryption: None,
                },
                BackupSchedule {
                    id: "disabled-backup".to_string(),
                    name: Some("Disabled Backup".to_string()),
                    cron: "0 2 * * *".to_string(),
                    target: BackupTarget::Database {
                        host: "localhost".to_string(),
                        port: Some(5432),
                        database: "testdb".to_string(),
                        user: Some("postgres".to_string()),
                    },
                    backup_type: BackupType::Full,
                    storage_profile: None,
                    enabled: false,
                    labels: HashMap::new(),
                    backup_dir: None,
                    encryption: None,
                },
            ],
            retention: vec![RetentionSchedule {
                id: "daily-retention".to_string(),
                name: Some("Daily Retention".to_string()),
                cron: "0 4 * * *".to_string(),
                policy_file: None,
                policy: None,
                storage_profile: None,
                enabled: true,
                apply: false,
                backup_dir: None,
            }],
            storage_profiles: vec![StorageProfile {
                name: "test-profile".to_string(),
                provider: "s3".to_string(),
                bucket: "test-bucket".to_string(),
                prefix: Some("backups/".to_string()),
                region: Some("us-east-1".to_string()),
                endpoint: Some("http://localhost:9000".to_string()),
                access_key: Some("minioadmin".to_string()),
                secret_key: Some("minioadmin".to_string()),
                encryption: None,
            }],
            default_backup_dir: Some("/tmp/backups".to_string()),
        }),
    }
}

#[test]
fn test_parsed_schedule_next_run() {
    // Test that we can parse a cron expression and get the next run time
    let schedule = ParsedSchedule::new("test".to_string(), "0 2 * * *").unwrap();
    let now = Utc::now();
    let next = schedule.next_after(now);

    assert!(next.is_some());
    let next_time = next.unwrap();
    assert!(next_time > now);

    // The next run should be at 2:00 AM
    assert_eq!(next_time.hour(), 2);
    assert_eq!(next_time.minute(), 0);
}

#[test]
fn test_parsed_schedule_next_n_runs() {
    let schedule = ParsedSchedule::new("test".to_string(), "0 * * * *").unwrap(); // Every hour
    let now = Utc::now();
    let next_runs = schedule.next_n_after(now, 5);

    assert_eq!(next_runs.len(), 5);

    // Each run should be 1 hour apart
    for i in 1..next_runs.len() {
        let diff = next_runs[i] - next_runs[i - 1];
        assert_eq!(diff.num_hours(), 1);
    }
}

#[test]
fn test_schedule_config_validation() {
    let config = create_test_config_with_schedules();
    let schedule_config = config.schedules.unwrap();

    // Should validate successfully
    assert!(schedule_config.validate().is_ok());
}

#[test]
fn test_schedule_config_duplicate_id_detection() {
    let config = ScheduleConfig {
        backups: vec![
            BackupSchedule {
                id: "duplicate".to_string(),
                name: None,
                cron: "0 2 * * *".to_string(),
                target: BackupTarget::Database {
                    host: "localhost".to_string(),
                    port: None,
                    database: "db1".to_string(),
                    user: None,
                },
                backup_type: BackupType::Snapshot,
                storage_profile: None,
                enabled: true,
                labels: HashMap::new(),
                backup_dir: None,
                encryption: None,
            },
            BackupSchedule {
                id: "duplicate".to_string(), // Same ID!
                name: None,
                cron: "0 3 * * *".to_string(),
                target: BackupTarget::Database {
                    host: "localhost".to_string(),
                    port: None,
                    database: "db2".to_string(),
                    user: None,
                },
                backup_type: BackupType::Snapshot,
                storage_profile: None,
                enabled: true,
                labels: HashMap::new(),
                backup_dir: None,
                encryption: None,
            },
        ],
        retention: vec![],
        storage_profiles: vec![],
        default_backup_dir: None,
    };

    // Should fail validation due to duplicate ID
    assert!(config.validate().is_err());
}

#[test]
fn test_schedule_config_invalid_cron() {
    let config = ScheduleConfig {
        backups: vec![BackupSchedule {
            id: "invalid-cron".to_string(),
            name: None,
            cron: "not a valid cron".to_string(),
            target: BackupTarget::Database {
                host: "localhost".to_string(),
                port: None,
                database: "db".to_string(),
                user: None,
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
        default_backup_dir: None,
    };

    // Should fail validation due to invalid cron
    assert!(config.validate().is_err());
}

#[test]
fn test_schedule_config_next_runs() {
    let config = create_test_config_with_schedules();
    let schedule_config = config.schedules.unwrap();

    let now = Utc::now();
    let runs = schedule_config.next_runs(now);

    // Should have runs for all schedules (including disabled ones)
    // We have 3 schedules: "every-minute", "disabled-backup", and "daily-retention"
    assert!(runs.len() >= 3);

    // Runs should be sorted by time
    for i in 1..runs.len() {
        assert!(runs[i].next_run >= runs[i - 1].next_run);
    }

    // Check that disabled schedule is included but marked as disabled
    let disabled_run = runs.iter().find(|r| r.schedule_id == "disabled-backup");
    assert!(disabled_run.is_some());
    assert!(!disabled_run.unwrap().enabled);
}

#[test]
fn test_scheduler_creation() {
    let config = Arc::new(Mutex::new(create_test_config_with_schedules()));
    let options = SchedulerOptions {
        check_interval_secs: 60,
        tolerance_secs: 30,
        dry_run: true,
        default_backup_dir: std::path::PathBuf::from("/tmp/backups"),
    };

    let scheduler = Scheduler::new(config, options);
    let runs = scheduler.next_runs();

    // Should have upcoming runs
    assert!(!runs.is_empty());
}

#[test]
fn test_scheduler_dry_run_mode() {
    let config = Arc::new(Mutex::new(create_test_config_with_schedules()));
    let options = SchedulerOptions {
        check_interval_secs: 1,
        tolerance_secs: 30,
        dry_run: true, // Dry-run mode
        default_backup_dir: std::path::PathBuf::from("/tmp/backups"),
    };

    let scheduler = Scheduler::new(config, options);

    // In dry-run mode, the scheduler should not actually execute backups
    // This is a basic sanity check
    assert!(scheduler.next_runs().len() >= 2);
}

#[test]
fn test_enabled_schedules_filter() {
    let config = create_test_config_with_schedules();
    let schedule_config = config.schedules.unwrap();

    let enabled_backups: Vec<_> = schedule_config.enabled_backup_schedules().collect();
    let enabled_retention: Vec<_> = schedule_config.enabled_retention_schedules().collect();

    // Should only include enabled schedules
    assert_eq!(enabled_backups.len(), 1); // Only "every-minute" is enabled
    assert_eq!(enabled_backups[0].id, "every-minute");

    assert_eq!(enabled_retention.len(), 1);
    assert_eq!(enabled_retention[0].id, "daily-retention");
}

#[test]
fn test_storage_profile_lookup() {
    let config = create_test_config_with_schedules();
    let schedule_config = config.schedules.unwrap();

    // Should find existing profile
    let profile = schedule_config.get_storage_profile("test-profile");
    assert!(profile.is_some());
    assert_eq!(profile.unwrap().bucket, "test-bucket");

    // Should not find non-existent profile
    let missing = schedule_config.get_storage_profile("non-existent");
    assert!(missing.is_none());
}

#[test]
fn test_cron_5_field_normalization() {
    // Standard 5-field cron should work
    let schedule = ParsedSchedule::new("test".to_string(), "0 2 * * *");
    assert!(schedule.is_ok());

    // 6-field cron should also work
    let schedule = ParsedSchedule::new("test".to_string(), "0 0 2 * * *");
    assert!(schedule.is_ok());
}

#[test]
fn test_should_run_at_tolerance() {
    let schedule = ParsedSchedule::new("test".to_string(), "0 0 2 * * *").unwrap();

    // Get the next run time
    let now = Utc::now();
    let next = schedule.next_after(now).unwrap();

    // Should run at exactly the scheduled time
    assert!(schedule.should_run_at(next, 30));

    // Should run within tolerance
    assert!(schedule.should_run_at(next + Duration::seconds(15), 30));
    assert!(schedule.should_run_at(next - Duration::seconds(15), 30));

    // Should not run outside tolerance
    assert!(!schedule.should_run_at(next + Duration::seconds(60), 30));
}

use chrono::Timelike;
