//! Retention policy definitions with PITR awareness.

use chrono::Duration;
use serde::{Deserialize, Serialize};

/// PITR-aware retention policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitrRetentionPolicy {
    /// Policy schema version
    pub version: String,
    /// Whether the policy is enabled
    pub enabled: bool,
    /// Retention rules (applied in order)
    pub rules: Vec<RetentionRule>,
    /// WAL retention configuration
    pub wal_retention: WalRetentionConfig,
    /// Safety settings
    pub safety: SafetySettings,
    /// Scope of the policy
    pub scope: RetentionScope,
}

/// A single retention rule
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RetentionRule {
    /// Keep backups within a time window
    KeepWithinDays {
        /// Number of days to keep backups
        days: u32,
        /// Minimum number of backups to keep regardless of age
        minimum: usize,
    },
    /// Keep the N most recent backups
    KeepLatest {
        /// Number of backups to keep
        count: usize,
    },
    /// Keep backups at specific intervals (GFS-style)
    KeepIntervals {
        /// Hourly backups to keep (within first N hours)
        hourly: Option<IntervalSpec>,
        /// Daily backups to keep
        daily: Option<IntervalSpec>,
        /// Weekly backups to keep
        weekly: Option<IntervalSpec>,
        /// Monthly backups to keep
        monthly: Option<IntervalSpec>,
        /// Yearly backups to keep
        yearly: Option<IntervalSpec>,
    },
    /// Keep backups matching specific tags
    KeepTagged {
        /// Tags that mark backups for retention
        tags: Vec<String>,
    },
    /// Keep all pinned backups (always applied)
    KeepPinned,
}

/// Specification for interval-based retention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntervalSpec {
    /// Number of backups to keep at this interval
    pub count: usize,
    /// Maximum age in days (backups older than this are not considered)
    pub max_age_days: Option<u32>,
}

/// WAL retention configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalRetentionConfig {
    /// Minimum PITR window to maintain (in hours)
    /// WAL segments needed to reach any point within this window are kept
    pub pitr_window_hours: u32,
    /// Keep WAL segments for all retained backups
    /// If true, WAL from the oldest retained backup to now is kept
    pub keep_for_retained_backups: bool,
    /// Maximum WAL age in days (hard limit)
    pub max_wal_age_days: Option<u32>,
    /// Maximum WAL size in GB (soft limit, triggers warning)
    pub max_wal_size_gb: Option<u64>,
}

/// Safety settings for retention operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetySettings {
    /// Perform dry run by default
    pub dry_run_by_default: bool,
    /// Require user confirmation for destructive operations
    pub require_confirmation: bool,
    /// Minimum number of successful backups to keep
    pub min_successful_backups: usize,
    /// Preserve backup chains (don't orphan incrementals)
    pub preserve_chains: bool,
    /// Never delete the most recent successful backup
    pub keep_latest_successful: bool,
    /// Minimum PITR window to maintain (hours)
    /// Purge will fail if it would reduce PITR window below this
    pub min_pitr_window_hours: Option<u32>,
}

/// Scope of the retention policy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct RetentionScope {
    /// Apply to specific databases (empty = all)
    pub databases: Vec<String>,
    /// Apply to specific backup types (empty = all)
    pub backup_types: Vec<String>,
    /// Exclude backups with these tags
    pub exclude_tags: Vec<String>,
}

impl Default for PitrRetentionPolicy {
    fn default() -> Self {
        Self {
            version: "1.0".to_string(),
            enabled: true,
            rules: vec![
                RetentionRule::KeepPinned,
                RetentionRule::KeepLatest { count: 3 },
                RetentionRule::KeepWithinDays {
                    days: 7,
                    minimum: 1,
                },
            ],
            wal_retention: WalRetentionConfig::default(),
            safety: SafetySettings::default(),
            scope: RetentionScope::default(),
        }
    }
}

impl Default for WalRetentionConfig {
    fn default() -> Self {
        Self {
            pitr_window_hours: 24, // 1 day PITR window
            keep_for_retained_backups: true,
            max_wal_age_days: Some(30),
            max_wal_size_gb: None,
        }
    }
}

impl Default for SafetySettings {
    fn default() -> Self {
        Self {
            dry_run_by_default: true,
            require_confirmation: true,
            min_successful_backups: 1,
            preserve_chains: true,
            keep_latest_successful: true,
            min_pitr_window_hours: Some(24),
        }
    }
}


impl PitrRetentionPolicy {
    /// Creates a new policy with sensible defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an aggressive policy (short retention)
    pub fn aggressive() -> Self {
        Self {
            version: "1.0".to_string(),
            enabled: true,
            rules: vec![
                RetentionRule::KeepPinned,
                RetentionRule::KeepLatest { count: 3 },
                RetentionRule::KeepWithinDays {
                    days: 7,
                    minimum: 1,
                },
            ],
            wal_retention: WalRetentionConfig {
                pitr_window_hours: 24,
                keep_for_retained_backups: true,
                max_wal_age_days: Some(7),
                max_wal_size_gb: Some(50),
            },
            safety: SafetySettings {
                min_successful_backups: 1,
                min_pitr_window_hours: Some(12),
                ..Default::default()
            },
            scope: RetentionScope::default(),
        }
    }

    /// Creates a conservative policy (long retention)
    pub fn conservative() -> Self {
        Self {
            version: "1.0".to_string(),
            enabled: true,
            rules: vec![
                RetentionRule::KeepPinned,
                RetentionRule::KeepLatest { count: 10 },
                RetentionRule::KeepIntervals {
                    hourly: Some(IntervalSpec {
                        count: 24,
                        max_age_days: Some(1),
                    }),
                    daily: Some(IntervalSpec {
                        count: 30,
                        max_age_days: Some(30),
                    }),
                    weekly: Some(IntervalSpec {
                        count: 52,
                        max_age_days: Some(365),
                    }),
                    monthly: Some(IntervalSpec {
                        count: 24,
                        max_age_days: Some(730),
                    }),
                    yearly: Some(IntervalSpec {
                        count: 10,
                        max_age_days: None,
                    }),
                },
            ],
            wal_retention: WalRetentionConfig {
                pitr_window_hours: 168, // 7 days
                keep_for_retained_backups: true,
                max_wal_age_days: Some(90),
                max_wal_size_gb: Some(500),
            },
            safety: SafetySettings {
                min_successful_backups: 3,
                min_pitr_window_hours: Some(72),
                ..Default::default()
            },
            scope: RetentionScope::default(),
        }
    }

    /// Creates a standard GFS (Grandfather-Father-Son) policy
    pub fn gfs_standard() -> Self {
        Self {
            version: "1.0".to_string(),
            enabled: true,
            rules: vec![
                RetentionRule::KeepPinned,
                RetentionRule::KeepIntervals {
                    hourly: None,
                    daily: Some(IntervalSpec {
                        count: 7,
                        max_age_days: Some(7),
                    }),
                    weekly: Some(IntervalSpec {
                        count: 4,
                        max_age_days: Some(30),
                    }),
                    monthly: Some(IntervalSpec {
                        count: 12,
                        max_age_days: Some(365),
                    }),
                    yearly: Some(IntervalSpec {
                        count: 7,
                        max_age_days: None,
                    }),
                },
            ],
            wal_retention: WalRetentionConfig {
                pitr_window_hours: 48,
                keep_for_retained_backups: true,
                max_wal_age_days: Some(30),
                max_wal_size_gb: Some(100),
            },
            safety: SafetySettings::default(),
            scope: RetentionScope::default(),
        }
    }

    /// Validates the policy for logical consistency
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.rules.is_empty() {
            errors.push("Policy must have at least one retention rule".to_string());
        }

        if self.safety.min_successful_backups == 0 {
            errors.push("min_successful_backups must be at least 1".to_string());
        }

        if self.wal_retention.pitr_window_hours == 0 {
            errors.push("pitr_window_hours must be greater than 0".to_string());
        }

        if let Some(min_pitr) = self.safety.min_pitr_window_hours {
            if min_pitr > self.wal_retention.pitr_window_hours {
                errors.push(format!(
                    "min_pitr_window_hours ({}) cannot exceed pitr_window_hours ({})",
                    min_pitr, self.wal_retention.pitr_window_hours
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Returns the PITR window as a Duration
    pub fn pitr_window(&self) -> Duration {
        Duration::hours(self.wal_retention.pitr_window_hours as i64)
    }

    /// Returns the minimum PITR window as a Duration
    pub fn min_pitr_window(&self) -> Option<Duration> {
        self.safety
            .min_pitr_window_hours
            .map(|h| Duration::hours(h as i64))
    }
}

impl WalRetentionConfig {
    /// Returns the PITR window as a Duration
    pub fn pitr_window(&self) -> Duration {
        Duration::hours(self.pitr_window_hours as i64)
    }

    /// Returns the max WAL age as a Duration
    pub fn max_age(&self) -> Option<Duration> {
        self.max_wal_age_days.map(|d| Duration::days(d as i64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy_is_valid() {
        let policy = PitrRetentionPolicy::default();
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_aggressive_policy_is_valid() {
        let policy = PitrRetentionPolicy::aggressive();
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_conservative_policy_is_valid() {
        let policy = PitrRetentionPolicy::conservative();
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn test_gfs_policy_is_valid() {
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

    #[test]
    fn test_pitr_window_duration() {
        let policy = PitrRetentionPolicy {
            wal_retention: WalRetentionConfig {
                pitr_window_hours: 48,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(policy.pitr_window(), Duration::hours(48));
    }
}
