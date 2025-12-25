//! Status types for observability.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Overall health status indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// Everything is healthy
    Healthy,
    /// Some warnings but operational
    Warning,
    /// Critical issues detected
    Critical,
    /// Status unknown or cannot be determined
    Unknown,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Warning => write!(f, "warning"),
            HealthStatus::Critical => write!(f, "critical"),
            HealthStatus::Unknown => write!(f, "unknown"),
        }
    }
}

impl HealthStatus {
    /// Returns ANSI color code for terminal output.
    pub fn color_code(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "\x1b[32m",  // Green
            HealthStatus::Warning => "\x1b[33m",  // Yellow
            HealthStatus::Critical => "\x1b[31m", // Red
            HealthStatus::Unknown => "\x1b[90m",  // Gray
        }
    }

    /// Returns emoji for status.
    pub fn emoji(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "✓",
            HealthStatus::Warning => "⚠",
            HealthStatus::Critical => "✗",
            HealthStatus::Unknown => "?",
        }
    }
}

/// High-level status summary for a PostgreSQL node/database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverallStatus {
    /// Timestamp when status was collected
    pub collected_at: DateTime<Utc>,
    /// Overall health status
    pub health: HealthStatus,
    /// Backup status summary
    pub backup: BackupStatus,
    /// PITR status summary
    pub pitr: PitrStatus,
    /// Retention status summary
    pub retention: RetentionStatus,
    /// Schedule status (if schedules are configured)
    pub schedules: Option<ScheduleStatus>,
    /// Storage usage summary
    pub storage: StorageStatus,
    /// List of issues/warnings
    pub issues: Vec<StatusIssue>,
}

impl OverallStatus {
    /// Compute overall health from component statuses.
    pub fn compute_health(&mut self) {
        let mut health = HealthStatus::Healthy;

        // Check backup health
        if self.backup.health == HealthStatus::Critical {
            health = HealthStatus::Critical;
        } else if self.backup.health == HealthStatus::Warning && health != HealthStatus::Critical {
            health = HealthStatus::Warning;
        }

        // Check PITR health
        if self.pitr.health == HealthStatus::Critical {
            health = HealthStatus::Critical;
        } else if self.pitr.health == HealthStatus::Warning && health != HealthStatus::Critical {
            health = HealthStatus::Warning;
        }

        // Check retention health
        if self.retention.health == HealthStatus::Critical {
            health = HealthStatus::Critical;
        } else if self.retention.health == HealthStatus::Warning && health != HealthStatus::Critical
        {
            health = HealthStatus::Warning;
        }

        self.health = health;
    }
}

/// Backup status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStatus {
    /// Health status of backups
    pub health: HealthStatus,
    /// Last successful backup info
    pub last_successful: Option<BackupInfo>,
    /// Last backup attempt (may have failed)
    pub last_attempt: Option<BackupInfo>,
    /// Total number of backups available
    pub total_backups: usize,
    /// Number of successful backups
    pub successful_backups: usize,
    /// Number of failed backups
    pub failed_backups: usize,
    /// Age of the most recent successful backup
    pub last_backup_age: Option<Duration>,
    /// Backup frequency (average interval between backups)
    pub average_interval: Option<Duration>,
    /// Number of encrypted backups
    pub encrypted_backups: usize,
    /// Number of unencrypted backups
    pub unencrypted_backups: usize,
    /// Issues related to backups
    pub issues: Vec<String>,
}

impl Default for BackupStatus {
    fn default() -> Self {
        Self {
            health: HealthStatus::Unknown,
            last_successful: None,
            last_attempt: None,
            total_backups: 0,
            successful_backups: 0,
            failed_backups: 0,
            last_backup_age: None,
            average_interval: None,
            encrypted_backups: 0,
            unencrypted_backups: 0,
            issues: Vec::new(),
        }
    }
}

/// Information about a specific backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    /// Backup ID
    pub id: String,
    /// Backup type (full, incremental, snapshot)
    pub backup_type: String,
    /// When the backup started
    pub start_time: DateTime<Utc>,
    /// When the backup completed
    pub end_time: Option<DateTime<Utc>>,
    /// Backup size in bytes
    pub size_bytes: u64,
    /// Whether backup completed successfully
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Location (local path or remote URL)
    pub location: Option<String>,
    /// Database name
    pub database: Option<String>,
    /// Whether the backup is encrypted
    pub encrypted: bool,
    /// Encryption algorithm used (if encrypted)
    pub encryption_algorithm: Option<String>,
}

/// PITR (Point-in-Time Recovery) status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitrStatus {
    /// Health status of PITR capability
    pub health: HealthStatus,
    /// Whether PITR is available
    pub available: bool,
    /// Earliest point in time recoverable
    pub earliest_recovery_point: Option<DateTime<Utc>>,
    /// Latest point in time recoverable (usually now or near-now)
    pub latest_recovery_point: Option<DateTime<Utc>>,
    /// Size of the recovery window
    pub recovery_window: Option<Duration>,
    /// Number of WAL segments available
    pub wal_segment_count: usize,
    /// Total size of WAL segments
    pub wal_size_bytes: u64,
    /// Number of base backups available for PITR
    pub base_backup_count: usize,
    /// Any gaps in WAL coverage
    pub wal_gaps: Vec<WalGap>,
    /// Issues related to PITR
    pub issues: Vec<String>,
}

impl Default for PitrStatus {
    fn default() -> Self {
        Self {
            health: HealthStatus::Unknown,
            available: false,
            earliest_recovery_point: None,
            latest_recovery_point: None,
            recovery_window: None,
            wal_segment_count: 0,
            wal_size_bytes: 0,
            base_backup_count: 0,
            wal_gaps: Vec::new(),
            issues: Vec::new(),
        }
    }
}

/// Represents a gap in WAL coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalGap {
    /// Start of the gap (LSN or segment name)
    pub start: String,
    /// End of the gap
    pub end: String,
    /// Approximate time range of the gap
    pub time_range: Option<(DateTime<Utc>, DateTime<Utc>)>,
}

/// Retention policy status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionStatus {
    /// Health status of retention
    pub health: HealthStatus,
    /// Whether a retention policy is configured
    pub policy_configured: bool,
    /// Policy name or description
    pub policy_name: Option<String>,
    /// Next scheduled purge time
    pub next_purge: Option<DateTime<Utc>>,
    /// Last purge execution time
    pub last_purge: Option<DateTime<Utc>>,
    /// Backups marked for deletion in next purge
    pub pending_deletions: usize,
    /// Estimated space to be freed (bytes)
    pub pending_space_freed: u64,
    /// Configured PITR window (hours)
    pub pitr_window_hours: Option<u32>,
    /// Minimum backups to keep
    pub min_backups_to_keep: Option<usize>,
    /// Issues related to retention
    pub issues: Vec<String>,
}

impl Default for RetentionStatus {
    fn default() -> Self {
        Self {
            health: HealthStatus::Unknown,
            policy_configured: false,
            policy_name: None,
            next_purge: None,
            last_purge: None,
            pending_deletions: 0,
            pending_space_freed: 0,
            pitr_window_hours: None,
            min_backups_to_keep: None,
            issues: Vec::new(),
        }
    }
}

/// Schedule status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleStatus {
    /// Health status of schedules
    pub health: HealthStatus,
    /// Number of backup schedules configured
    pub backup_schedules: usize,
    /// Number of enabled backup schedules
    pub enabled_backup_schedules: usize,
    /// Number of retention schedules configured
    pub retention_schedules: usize,
    /// Number of enabled retention schedules
    pub enabled_retention_schedules: usize,
    /// Next scheduled backup
    pub next_backup: Option<ScheduledTask>,
    /// Next scheduled retention run
    pub next_retention: Option<ScheduledTask>,
    /// Issues related to schedules
    pub issues: Vec<String>,
}

impl Default for ScheduleStatus {
    fn default() -> Self {
        Self {
            health: HealthStatus::Unknown,
            backup_schedules: 0,
            enabled_backup_schedules: 0,
            retention_schedules: 0,
            enabled_retention_schedules: 0,
            next_backup: None,
            next_retention: None,
            issues: Vec::new(),
        }
    }
}

/// Information about a scheduled task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    /// Schedule ID
    pub schedule_id: String,
    /// Schedule name
    pub name: Option<String>,
    /// Next run time
    pub next_run: DateTime<Utc>,
    /// Time until next run
    pub time_until: Duration,
    /// Cron expression
    pub cron: String,
}

/// Storage usage status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStatus {
    /// Health status of storage
    pub health: HealthStatus,
    /// Local storage usage
    pub local: Option<StorageUsage>,
    /// Remote storage usage
    pub remote: Option<StorageUsage>,
    /// Issues related to storage
    pub issues: Vec<String>,
}

impl Default for StorageStatus {
    fn default() -> Self {
        Self {
            health: HealthStatus::Unknown,
            local: None,
            remote: None,
            issues: Vec::new(),
        }
    }
}

/// Storage usage details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageUsage {
    /// Total size used (bytes)
    pub used_bytes: u64,
    /// Number of backup files/objects
    pub backup_count: usize,
    /// Number of WAL files/objects
    pub wal_count: usize,
    /// Size of backups (bytes)
    pub backup_size_bytes: u64,
    /// Size of WAL (bytes)
    pub wal_size_bytes: u64,
    /// Location (path or bucket)
    pub location: String,
}

/// An issue or warning in the status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusIssue {
    /// Severity of the issue
    pub severity: HealthStatus,
    /// Category (backup, pitr, retention, storage, schedule)
    pub category: String,
    /// Issue message
    pub message: String,
    /// Suggested action
    pub suggestion: Option<String>,
}

/// Counters for metrics tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationCounters {
    /// Successful backup count
    pub backups_successful: u64,
    /// Failed backup count
    pub backups_failed: u64,
    /// Successful restore count
    pub restores_successful: u64,
    /// Failed restore count
    pub restores_failed: u64,
    /// Successful PITR count
    pub pitr_successful: u64,
    /// Failed PITR count
    pub pitr_failed: u64,
    /// Successful purge count
    pub purges_successful: u64,
    /// Failed purge count
    pub purges_failed: u64,
}

/// Gauge values for metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricGauges {
    /// Age of latest backup in seconds
    pub latest_backup_age_seconds: Option<f64>,
    /// PITR window size in seconds
    pub pitr_window_seconds: Option<f64>,
    /// Total backup storage used in bytes
    pub backup_storage_bytes: u64,
    /// Total WAL storage used in bytes
    pub wal_storage_bytes: u64,
    /// Number of available backups
    pub available_backups: u64,
    /// Number of WAL segments
    pub wal_segments: u64,
    /// Number of encrypted backups
    pub encrypted_backups: u64,
    /// Number of unencrypted backups
    pub unencrypted_backups: u64,
}

/// Labels for metrics (database, host, etc.)
pub type MetricLabels = HashMap<String, String>;

// ============================================================================
// Performance Metrics Types
// ============================================================================

/// Performance metrics for backup operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupPerformanceMetrics {
    /// Total number of backups measured
    pub sample_count: u64,
    /// Average backup duration in seconds
    pub avg_duration_seconds: f64,
    /// Minimum backup duration in seconds
    pub min_duration_seconds: Option<f64>,
    /// Maximum backup duration in seconds
    pub max_duration_seconds: Option<f64>,
    /// Average backup size in bytes
    pub avg_size_bytes: u64,
    /// Average throughput in bytes per second
    pub avg_throughput_bytes_per_sec: f64,
    /// Last backup duration in seconds
    pub last_duration_seconds: Option<f64>,
    /// Last backup size in bytes
    pub last_size_bytes: Option<u64>,
    /// Last backup throughput in bytes per second
    pub last_throughput_bytes_per_sec: Option<f64>,
}

impl BackupPerformanceMetrics {
    /// Record a new backup measurement.
    pub fn record(&mut self, duration_seconds: f64, size_bytes: u64) {
        let throughput = if duration_seconds > 0.0 {
            size_bytes as f64 / duration_seconds
        } else {
            0.0
        };

        // Update last values
        self.last_duration_seconds = Some(duration_seconds);
        self.last_size_bytes = Some(size_bytes);
        self.last_throughput_bytes_per_sec = Some(throughput);

        // Update min/max
        self.min_duration_seconds = Some(
            self.min_duration_seconds
                .map(|m| m.min(duration_seconds))
                .unwrap_or(duration_seconds),
        );
        self.max_duration_seconds = Some(
            self.max_duration_seconds
                .map(|m| m.max(duration_seconds))
                .unwrap_or(duration_seconds),
        );

        // Update averages (running average)
        let old_count = self.sample_count as f64;
        let new_count = old_count + 1.0;

        self.avg_duration_seconds =
            (self.avg_duration_seconds * old_count + duration_seconds) / new_count;
        self.avg_size_bytes =
            ((self.avg_size_bytes as f64 * old_count + size_bytes as f64) / new_count) as u64;
        self.avg_throughput_bytes_per_sec =
            (self.avg_throughput_bytes_per_sec * old_count + throughput) / new_count;

        self.sample_count += 1;
    }
}

/// Performance metrics for PITR operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PitrPerformanceMetrics {
    /// Total number of PITR operations measured
    pub sample_count: u64,
    /// Average PITR duration in seconds
    pub avg_duration_seconds: f64,
    /// Average WAL replay rate in bytes per second
    pub avg_wal_replay_rate_bytes_per_sec: f64,
    /// Last PITR duration in seconds
    pub last_duration_seconds: Option<f64>,
    /// Last WAL bytes replayed
    pub last_wal_bytes_replayed: Option<u64>,
    /// Last WAL replay rate in bytes per second
    pub last_wal_replay_rate_bytes_per_sec: Option<f64>,
}

impl PitrPerformanceMetrics {
    /// Record a new PITR measurement.
    pub fn record(&mut self, duration_seconds: f64, wal_bytes_replayed: u64) {
        let replay_rate = if duration_seconds > 0.0 {
            wal_bytes_replayed as f64 / duration_seconds
        } else {
            0.0
        };

        // Update last values
        self.last_duration_seconds = Some(duration_seconds);
        self.last_wal_bytes_replayed = Some(wal_bytes_replayed);
        self.last_wal_replay_rate_bytes_per_sec = Some(replay_rate);

        // Update averages
        let old_count = self.sample_count as f64;
        let new_count = old_count + 1.0;

        self.avg_duration_seconds =
            (self.avg_duration_seconds * old_count + duration_seconds) / new_count;
        self.avg_wal_replay_rate_bytes_per_sec =
            (self.avg_wal_replay_rate_bytes_per_sec * old_count + replay_rate) / new_count;

        self.sample_count += 1;
    }
}

/// Performance metrics for retention/purge operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetentionPerformanceMetrics {
    /// Total number of retention runs measured
    pub sample_count: u64,
    /// Average retention run duration in seconds
    pub avg_duration_seconds: f64,
    /// Average number of backups evaluated per run
    pub avg_backups_evaluated: f64,
    /// Average number of backups deleted per run
    pub avg_backups_deleted: f64,
    /// Average bytes freed per run
    pub avg_bytes_freed: u64,
    /// Last retention run duration in seconds
    pub last_duration_seconds: Option<f64>,
    /// Last number of backups evaluated
    pub last_backups_evaluated: Option<u64>,
    /// Last number of backups deleted
    pub last_backups_deleted: Option<u64>,
    /// Last bytes freed
    pub last_bytes_freed: Option<u64>,
}

impl RetentionPerformanceMetrics {
    /// Record a new retention run measurement.
    pub fn record(
        &mut self,
        duration_seconds: f64,
        backups_evaluated: u64,
        backups_deleted: u64,
        bytes_freed: u64,
    ) {
        // Update last values
        self.last_duration_seconds = Some(duration_seconds);
        self.last_backups_evaluated = Some(backups_evaluated);
        self.last_backups_deleted = Some(backups_deleted);
        self.last_bytes_freed = Some(bytes_freed);

        // Update averages
        let old_count = self.sample_count as f64;
        let new_count = old_count + 1.0;

        self.avg_duration_seconds =
            (self.avg_duration_seconds * old_count + duration_seconds) / new_count;
        self.avg_backups_evaluated =
            (self.avg_backups_evaluated * old_count + backups_evaluated as f64) / new_count;
        self.avg_backups_deleted =
            (self.avg_backups_deleted * old_count + backups_deleted as f64) / new_count;
        self.avg_bytes_freed =
            ((self.avg_bytes_freed as f64 * old_count + bytes_freed as f64) / new_count) as u64;

        self.sample_count += 1;
    }
}

/// Aggregated performance metrics for all operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Backup performance metrics
    pub backup: BackupPerformanceMetrics,
    /// PITR performance metrics
    pub pitr: PitrPerformanceMetrics,
    /// Retention performance metrics
    pub retention: RetentionPerformanceMetrics,
    /// Timestamp of last update
    pub last_updated: Option<DateTime<Utc>>,
}

impl PerformanceMetrics {
    /// Create new performance metrics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a backup operation.
    pub fn record_backup(&mut self, duration_seconds: f64, size_bytes: u64) {
        self.backup.record(duration_seconds, size_bytes);
        self.last_updated = Some(Utc::now());
    }

    /// Record a PITR operation.
    pub fn record_pitr(&mut self, duration_seconds: f64, wal_bytes_replayed: u64) {
        self.pitr.record(duration_seconds, wal_bytes_replayed);
        self.last_updated = Some(Utc::now());
    }

    /// Record a retention operation.
    pub fn record_retention(
        &mut self,
        duration_seconds: f64,
        backups_evaluated: u64,
        backups_deleted: u64,
        bytes_freed: u64,
    ) {
        self.retention.record(
            duration_seconds,
            backups_evaluated,
            backups_deleted,
            bytes_freed,
        );
        self.last_updated = Some(Utc::now());
    }

    /// Export performance metrics in Prometheus format.
    pub fn export_prometheus(&self, labels: &MetricLabels) -> String {
        let labels_str = format_prometheus_labels(labels);
        let mut output = String::new();

        // Backup metrics
        output.push_str("# HELP warden_backup_duration_seconds Backup operation duration\n");
        output.push_str("# TYPE warden_backup_duration_seconds gauge\n");
        if let Some(duration) = self.backup.last_duration_seconds {
            output.push_str(&format!(
                "warden_backup_duration_seconds{{{}}} {:.3}\n",
                labels_str, duration
            ));
        }

        output.push_str("# HELP warden_backup_size_bytes Backup size in bytes\n");
        output.push_str("# TYPE warden_backup_size_bytes gauge\n");
        if let Some(size) = self.backup.last_size_bytes {
            output.push_str(&format!(
                "warden_backup_size_bytes{{{}}} {}\n",
                labels_str, size
            ));
        }

        output.push_str("# HELP warden_backup_throughput_bytes_per_second Backup throughput\n");
        output.push_str("# TYPE warden_backup_throughput_bytes_per_second gauge\n");
        if let Some(throughput) = self.backup.last_throughput_bytes_per_sec {
            output.push_str(&format!(
                "warden_backup_throughput_bytes_per_second{{{}}} {:.2}\n",
                labels_str, throughput
            ));
        }

        output.push_str("# HELP warden_backup_avg_duration_seconds Average backup duration\n");
        output.push_str("# TYPE warden_backup_avg_duration_seconds gauge\n");
        output.push_str(&format!(
            "warden_backup_avg_duration_seconds{{{}}} {:.3}\n",
            labels_str, self.backup.avg_duration_seconds
        ));

        // PITR metrics
        output.push_str("# HELP warden_pitr_duration_seconds PITR operation duration\n");
        output.push_str("# TYPE warden_pitr_duration_seconds gauge\n");
        if let Some(duration) = self.pitr.last_duration_seconds {
            output.push_str(&format!(
                "warden_pitr_duration_seconds{{{}}} {:.3}\n",
                labels_str, duration
            ));
        }

        output.push_str("# HELP warden_pitr_wal_replay_rate_bytes_per_second WAL replay rate\n");
        output.push_str("# TYPE warden_pitr_wal_replay_rate_bytes_per_second gauge\n");
        if let Some(rate) = self.pitr.last_wal_replay_rate_bytes_per_sec {
            output.push_str(&format!(
                "warden_pitr_wal_replay_rate_bytes_per_second{{{}}} {:.2}\n",
                labels_str, rate
            ));
        }

        // Retention metrics
        output.push_str("# HELP warden_retention_duration_seconds Retention run duration\n");
        output.push_str("# TYPE warden_retention_duration_seconds gauge\n");
        if let Some(duration) = self.retention.last_duration_seconds {
            output.push_str(&format!(
                "warden_retention_duration_seconds{{{}}} {:.3}\n",
                labels_str, duration
            ));
        }

        output.push_str("# HELP warden_retention_backups_deleted Backups deleted in last run\n");
        output.push_str("# TYPE warden_retention_backups_deleted gauge\n");
        if let Some(deleted) = self.retention.last_backups_deleted {
            output.push_str(&format!(
                "warden_retention_backups_deleted{{{}}} {}\n",
                labels_str, deleted
            ));
        }

        output.push_str("# HELP warden_retention_bytes_freed Bytes freed in last run\n");
        output.push_str("# TYPE warden_retention_bytes_freed gauge\n");
        if let Some(freed) = self.retention.last_bytes_freed {
            output.push_str(&format!(
                "warden_retention_bytes_freed{{{}}} {}\n",
                labels_str, freed
            ));
        }

        output
    }
}

/// Format labels for Prometheus output.
fn format_prometheus_labels(labels: &MetricLabels) -> String {
    if labels.is_empty() {
        return String::new();
    }

    labels
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, v))
        .collect::<Vec<_>>()
        .join(",")
}

/// Format helpers for human-readable output.
pub fn format_duration(duration: Duration) -> String {
    let total_secs = duration.num_seconds();
    if total_secs < 60 {
        format!("{}s", total_secs)
    } else if total_secs < 3600 {
        format!("{}m {}s", total_secs / 60, total_secs % 60)
    } else if total_secs < 86400 {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    } else {
        let days = total_secs / 86400;
        let hours = (total_secs % 86400) / 3600;
        format!("{}d {}h", days, hours)
    }
}

/// Format bytes as human-readable size.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::seconds(30)), "30s");
        assert_eq!(format_duration(Duration::seconds(90)), "1m 30s");
        assert_eq!(format_duration(Duration::seconds(3661)), "1h 1m");
        assert_eq!(format_duration(Duration::seconds(90061)), "1d 1h");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn test_health_status_display() {
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(HealthStatus::Warning.to_string(), "warning");
        assert_eq!(HealthStatus::Critical.to_string(), "critical");
    }
}
