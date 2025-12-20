//! Schedule configuration and cron parsing for backup and retention tasks.
//!
//! This module provides types and utilities for defining schedules that trigger
//! backup and retention operations automatically.

use chrono::{DateTime, Utc};
use cron::Schedule as CronSchedule;
use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

use crate::encryption::EncryptionConfig;

/// Errors that can occur when working with schedules.
#[derive(Error, Debug)]
pub enum ScheduleError {
    #[error("Invalid cron expression '{expression}': {reason}")]
    InvalidCronExpression { expression: String, reason: String },

    #[error("Schedule '{id}' not found")]
    ScheduleNotFound { id: String },

    #[error("Invalid schedule configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Duplicate schedule ID: {0}")]
    DuplicateScheduleId(String),
}

/// Type of backup to perform.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackupType {
    /// Full backup using pg_basebackup
    Full,
    /// Incremental backup
    Incremental,
    /// Snapshot backup (physical + logical)
    #[default]
    Snapshot,
}

impl std::fmt::Display for BackupType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupType::Full => write!(f, "full"),
            BackupType::Incremental => write!(f, "incremental"),
            BackupType::Snapshot => write!(f, "snapshot"),
        }
    }
}

/// Target specification for a backup schedule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum BackupTarget {
    /// Target a specific cluster by ID
    Cluster { cluster_id: String },
    /// Target a specific node by ID
    Node { node_id: String },
    /// Target a specific database on a host
    Database {
        host: String,
        port: Option<u16>,
        database: String,
        user: Option<String>,
    },
}

/// Configuration for a scheduled backup task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSchedule {
    /// Unique identifier for this schedule
    pub id: String,

    /// Human-readable name/description
    #[serde(default)]
    pub name: Option<String>,

    /// Cron expression defining when to run (e.g., "0 0 2 * * *" for 2 AM daily)
    pub cron: String,

    /// Target for the backup (cluster, node, or database)
    pub target: BackupTarget,

    /// Type of backup to perform
    #[serde(default)]
    pub backup_type: BackupType,

    /// Storage profile name (references storage_profiles in config)
    #[serde(default)]
    pub storage_profile: Option<String>,

    /// Whether this schedule is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Labels to attach to backups created by this schedule
    #[serde(default)]
    pub labels: HashMap<String, String>,

    /// Backup directory override (uses default if not specified)
    #[serde(default)]
    pub backup_dir: Option<String>,

    /// Encryption configuration for this schedule (overrides storage profile settings)
    #[serde(default)]
    pub encryption: Option<EncryptionConfig>,
}

/// Configuration for a scheduled retention task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionSchedule {
    /// Unique identifier for this schedule
    pub id: String,

    /// Human-readable name/description
    #[serde(default)]
    pub name: Option<String>,

    /// Cron expression defining when to run
    pub cron: String,

    /// Path to retention policy file
    #[serde(default)]
    pub policy_file: Option<String>,

    /// Inline retention policy (alternative to policy_file)
    #[serde(default)]
    pub policy: Option<RetentionPolicy>,

    /// Storage profile name for remote storage operations
    #[serde(default)]
    pub storage_profile: Option<String>,

    /// Whether this schedule is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Whether to actually apply deletions (false = dry-run only)
    #[serde(default)]
    pub apply: bool,

    /// Backup directory to evaluate
    #[serde(default)]
    pub backup_dir: Option<String>,
}

/// Inline retention policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Number of daily backups to keep
    #[serde(default)]
    pub keep_daily: Option<u32>,

    /// Number of weekly backups to keep
    #[serde(default)]
    pub keep_weekly: Option<u32>,

    /// Number of monthly backups to keep
    #[serde(default)]
    pub keep_monthly: Option<u32>,

    /// Minimum age in days before a backup can be deleted
    #[serde(default)]
    pub min_age_days: Option<u32>,

    /// Maximum age in days after which backups are deleted
    #[serde(default)]
    pub max_age_days: Option<u32>,
}

/// Storage profile for S3-compatible storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageProfile {
    /// Profile name
    pub name: String,

    /// Storage provider type (s3, minio)
    #[serde(default = "default_provider")]
    pub provider: String,

    /// S3 bucket name
    pub bucket: String,

    /// S3 key prefix
    #[serde(default)]
    pub prefix: Option<String>,

    /// AWS region
    #[serde(default)]
    pub region: Option<String>,

    /// Custom endpoint URL (for MinIO, etc.)
    #[serde(default)]
    pub endpoint: Option<String>,

    /// Access key (or env var reference like "env:AWS_ACCESS_KEY_ID")
    #[serde(default)]
    pub access_key: Option<String>,

    /// Secret key (or env var reference like "env:AWS_SECRET_ACCESS_KEY")
    #[serde(default)]
    pub secret_key: Option<String>,

    /// Encryption configuration for backups stored with this profile
    #[serde(default)]
    pub encryption: Option<EncryptionConfig>,
}

/// Top-level schedule configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScheduleConfig {
    /// Backup schedules
    #[serde(default)]
    pub backups: Vec<BackupSchedule>,

    /// Retention schedules
    #[serde(default)]
    pub retention: Vec<RetentionSchedule>,

    /// Named storage profiles
    #[serde(default)]
    pub storage_profiles: Vec<StorageProfile>,

    /// Default backup directory
    #[serde(default)]
    pub default_backup_dir: Option<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_provider() -> String {
    "s3".to_string()
}

/// Parsed schedule with computed next run times.
#[derive(Debug, Clone)]
pub struct ParsedSchedule {
    /// Original schedule ID
    pub id: String,
    /// Parsed cron schedule
    cron_schedule: CronSchedule,
}

impl ParsedSchedule {
    /// Parse a cron expression into a schedule.
    pub fn new(id: String, cron_expr: &str) -> Result<Self, ScheduleError> {
        // The cron crate expects 6 or 7 fields: sec min hour day month weekday [year]
        // If user provides 5 fields (standard cron), prepend "0" for seconds
        let normalized_expr = normalize_cron_expression(cron_expr);

        let cron_schedule = CronSchedule::from_str(&normalized_expr).map_err(|e| {
            ScheduleError::InvalidCronExpression {
                expression: cron_expr.to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(Self { id, cron_schedule })
    }

    /// Get the next scheduled run time after the given time.
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.cron_schedule.after(&after).next()
    }

    /// Get the next N scheduled run times after the given time.
    pub fn next_n_after(&self, after: DateTime<Utc>, n: usize) -> Vec<DateTime<Utc>> {
        self.cron_schedule.after(&after).take(n).collect()
    }

    /// Check if the schedule should run at the given time (within tolerance).
    pub fn should_run_at(&self, time: DateTime<Utc>, tolerance_secs: i64) -> bool {
        let tolerance = chrono::Duration::seconds(tolerance_secs);
        let window_start = time - tolerance;
        let window_end = time + tolerance;
        if let Some(next) = self.next_after(window_start) {
            next <= window_end
        } else {
            false
        }
    }
}

/// Normalize a cron expression to the 6-field format expected by the cron crate.
///
/// Standard cron uses 5 fields: min hour day month weekday
/// The cron crate uses 6 fields: sec min hour day month weekday
///
/// This function prepends "0" for seconds if only 5 fields are provided.
fn normalize_cron_expression(expr: &str) -> String {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() == 5 {
        // Standard 5-field cron, prepend seconds
        format!("0 {}", expr)
    } else {
        // Already 6 or 7 fields, use as-is
        expr.to_string()
    }
}

impl ScheduleConfig {
    /// Validate the schedule configuration.
    pub fn validate(&self) -> Result<(), ScheduleError> {
        // Check for duplicate backup schedule IDs
        let mut seen_ids = std::collections::HashSet::new();
        for schedule in &self.backups {
            if !seen_ids.insert(&schedule.id) {
                return Err(ScheduleError::DuplicateScheduleId(schedule.id.clone()));
            }
            // Validate cron expression
            ParsedSchedule::new(schedule.id.clone(), &schedule.cron)?;
        }

        // Check for duplicate retention schedule IDs
        for schedule in &self.retention {
            if !seen_ids.insert(&schedule.id) {
                return Err(ScheduleError::DuplicateScheduleId(schedule.id.clone()));
            }
            // Validate cron expression
            ParsedSchedule::new(schedule.id.clone(), &schedule.cron)?;
        }

        // Validate storage profile references
        let profile_names: std::collections::HashSet<_> = self
            .storage_profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect();

        for schedule in &self.backups {
            if let Some(ref profile) = schedule.storage_profile {
                if !profile_names.contains(profile.as_str()) {
                    return Err(ScheduleError::InvalidConfiguration(format!(
                        "Backup schedule '{}' references unknown storage profile '{}'",
                        schedule.id, profile
                    )));
                }
            }
        }

        for schedule in &self.retention {
            if let Some(ref profile) = schedule.storage_profile {
                if !profile_names.contains(profile.as_str()) {
                    return Err(ScheduleError::InvalidConfiguration(format!(
                        "Retention schedule '{}' references unknown storage profile '{}'",
                        schedule.id, profile
                    )));
                }
            }
        }

        Ok(())
    }

    /// Get a storage profile by name.
    pub fn get_storage_profile(&self, name: &str) -> Option<&StorageProfile> {
        self.storage_profiles.iter().find(|p| p.name == name)
    }

    /// Get all enabled backup schedules.
    pub fn enabled_backup_schedules(&self) -> impl Iterator<Item = &BackupSchedule> {
        self.backups.iter().filter(|s| s.enabled)
    }

    /// Get all enabled retention schedules.
    pub fn enabled_retention_schedules(&self) -> impl Iterator<Item = &RetentionSchedule> {
        self.retention.iter().filter(|s| s.enabled)
    }
}

/// Information about a scheduled run.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduledRun {
    /// Schedule ID
    pub schedule_id: String,
    /// Schedule name (if any)
    pub schedule_name: Option<String>,
    /// Type of schedule (backup or retention)
    pub schedule_type: ScheduleType,
    /// Next run time
    pub next_run: DateTime<Utc>,
    /// Whether the schedule is enabled
    pub enabled: bool,
}

/// Type of schedule.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleType {
    Backup,
    Retention,
}

impl std::fmt::Display for ScheduleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleType::Backup => write!(f, "backup"),
            ScheduleType::Retention => write!(f, "retention"),
        }
    }
}

impl ScheduleConfig {
    /// Get the next scheduled runs for all schedules.
    pub fn next_runs(&self, after: DateTime<Utc>) -> Vec<ScheduledRun> {
        let mut runs = Vec::new();

        for schedule in &self.backups {
            match ParsedSchedule::new(schedule.id.clone(), &schedule.cron) {
                Ok(parsed) => {
                    if let Some(next) = parsed.next_after(after) {
                        runs.push(ScheduledRun {
                            schedule_id: schedule.id.clone(),
                            schedule_name: schedule.name.clone(),
                            schedule_type: ScheduleType::Backup,
                            next_run: next,
                            enabled: schedule.enabled,
                        });
                    }
                }
                Err(e) => {
                    warn!("Failed to parse backup schedule '{}': {}", schedule.id, e);
                }
            }
        }

        for schedule in &self.retention {
            match ParsedSchedule::new(schedule.id.clone(), &schedule.cron) {
                Ok(parsed) => {
                    if let Some(next) = parsed.next_after(after) {
                        runs.push(ScheduledRun {
                            schedule_id: schedule.id.clone(),
                            schedule_name: schedule.name.clone(),
                            schedule_type: ScheduleType::Retention,
                            next_run: next,
                            enabled: schedule.enabled,
                        });
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to parse retention schedule '{}': {}",
                        schedule.id, e
                    );
                }
            }
        }

        // Sort by next run time
        runs.sort_by_key(|r| r.next_run);
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_cron_5_fields() {
        let expr = "0 2 * * *"; // 2 AM daily
        let normalized = normalize_cron_expression(expr);
        assert_eq!(normalized, "0 0 2 * * *");
    }

    #[test]
    fn test_normalize_cron_6_fields() {
        let expr = "0 0 2 * * *"; // Already 6 fields
        let normalized = normalize_cron_expression(expr);
        assert_eq!(normalized, "0 0 2 * * *");
    }

    #[test]
    fn test_parse_schedule_valid() {
        let schedule = ParsedSchedule::new("test".to_string(), "0 2 * * *");
        assert!(schedule.is_ok());
    }

    #[test]
    fn test_parse_schedule_invalid() {
        let schedule = ParsedSchedule::new("test".to_string(), "invalid cron");
        assert!(schedule.is_err());
    }

    #[test]
    fn test_next_run_computation() {
        let schedule = ParsedSchedule::new("test".to_string(), "0 0 2 * * *").unwrap();
        let now = Utc::now();
        let next = schedule.next_after(now);
        assert!(next.is_some());
        assert!(next.unwrap() > now);
    }

    #[test]
    fn test_schedule_config_validation() {
        let config = ScheduleConfig {
            backups: vec![BackupSchedule {
                id: "daily-backup".to_string(),
                name: Some("Daily Backup".to_string()),
                cron: "0 2 * * *".to_string(),
                target: BackupTarget::Database {
                    host: "localhost".to_string(),
                    port: Some(5432),
                    database: "mydb".to_string(),
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
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_duplicate_schedule_id() {
        let config = ScheduleConfig {
            backups: vec![
                BackupSchedule {
                    id: "same-id".to_string(),
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
                    id: "same-id".to_string(),
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

        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ScheduleError::DuplicateScheduleId(_)
        ));
    }

    #[test]
    fn test_backup_type_display() {
        assert_eq!(BackupType::Full.to_string(), "full");
        assert_eq!(BackupType::Incremental.to_string(), "incremental");
        assert_eq!(BackupType::Snapshot.to_string(), "snapshot");
    }
}
