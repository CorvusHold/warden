//! Core types for Point-in-Time Recovery.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Represents a recovery target specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryTarget {
    /// Recover to a specific timestamp (RFC3339 format).
    Time(DateTime<Utc>),
    /// Recover to a specific LSN (Log Sequence Number).
    Lsn(String),
    /// Recover to a named restore point.
    RestorePoint(String),
    /// Recover to the end of available WAL (latest possible point).
    Latest,
}

impl RecoveryTarget {
    /// Parse a recovery target from a string.
    /// Supports RFC3339 timestamps and LSN format (e.g., "0/16B3748").
    pub fn parse(s: &str) -> Result<Self, String> {
        // Try parsing as RFC3339 timestamp first
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(RecoveryTarget::Time(dt.with_timezone(&Utc)));
        }

        // Try parsing as LSN (format: X/XXXXXXXX)
        if s.contains('/') && s.chars().all(|c| c.is_ascii_hexdigit() || c == '/') {
            return Ok(RecoveryTarget::Lsn(s.to_string()));
        }

        // Check for special keywords
        if s.eq_ignore_ascii_case("latest") {
            return Ok(RecoveryTarget::Latest);
        }

        // Treat as restore point name
        Ok(RecoveryTarget::RestorePoint(s.to_string()))
    }

    /// Returns true if this is a time-based target.
    pub fn is_time_based(&self) -> bool {
        matches!(self, RecoveryTarget::Time(_))
    }

    /// Get the target time if this is a time-based target.
    pub fn as_time(&self) -> Option<DateTime<Utc>> {
        match self {
            RecoveryTarget::Time(t) => Some(*t),
            _ => None,
        }
    }
}

/// Information about a base backup that can be used for PITR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseBackupInfo {
    /// Unique backup identifier.
    pub id: Uuid,
    /// Path to the backup (local or S3 key prefix).
    pub path: String,
    /// When the backup started.
    pub start_time: DateTime<Utc>,
    /// When the backup completed.
    pub end_time: Option<DateTime<Utc>>,
    /// WAL position at backup start.
    pub wal_start: Option<String>,
    /// WAL position at backup end.
    pub wal_end: Option<String>,
    /// PostgreSQL server version.
    pub server_version: String,
    /// Size of the backup in bytes.
    pub size_bytes: u64,
    /// Whether this backup is stored remotely.
    pub is_remote: bool,
}

/// Information about a WAL segment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalSegmentInfo {
    /// WAL segment filename (e.g., "000000010000000000000001").
    pub filename: String,
    /// Timeline ID extracted from the filename.
    pub timeline_id: u32,
    /// Log segment number.
    pub log_id: u32,
    /// Segment number within the log.
    pub segment_id: u32,
    /// Size of the segment in bytes.
    pub size_bytes: u64,
    /// Last modified time (if available).
    pub last_modified: Option<DateTime<Utc>>,
    /// Path to the segment (local or S3 key).
    pub path: String,
    /// Whether this segment is stored remotely.
    pub is_remote: bool,
    /// Whether this segment is compressed.
    pub is_compressed: bool,
}

impl WalSegmentInfo {
    /// Parse WAL segment info from a filename.
    /// WAL filenames are 24 hex characters: TTTTTTTTLLLLLLLLSSSSSSSS
    /// where T=timeline, L=log, S=segment.
    pub fn parse_filename(
        filename: &str,
        path: String,
        size_bytes: u64,
        last_modified: Option<DateTime<Utc>>,
        is_remote: bool,
    ) -> Option<Self> {
        // Strip common extensions
        let base_name = filename
            .trim_end_matches(".gz")
            .trim_end_matches(".lz4")
            .trim_end_matches(".zst")
            .trim_end_matches(".partial");

        // WAL filenames are exactly 24 hex characters
        if base_name.len() != 24 || !base_name.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }

        let timeline_id = u32::from_str_radix(&base_name[0..8], 16).ok()?;
        let log_id = u32::from_str_radix(&base_name[8..16], 16).ok()?;
        let segment_id = u32::from_str_radix(&base_name[16..24], 16).ok()?;

        let is_compressed =
            filename.ends_with(".gz") || filename.ends_with(".lz4") || filename.ends_with(".zst");

        Some(Self {
            filename: filename.to_string(),
            timeline_id,
            log_id,
            segment_id,
            size_bytes,
            last_modified,
            path,
            is_remote,
            is_compressed,
        })
    }

    /// Get the LSN range covered by this segment.
    /// Each segment is 16MB (0x1000000 bytes).
    pub fn lsn_range(&self) -> (u64, u64) {
        const SEGMENT_SIZE: u64 = 16 * 1024 * 1024; // 16MB
        let start = ((self.log_id as u64) << 32) | ((self.segment_id as u64) * SEGMENT_SIZE);
        let end = start + SEGMENT_SIZE - 1;
        (start, end)
    }

    /// Format LSN as PostgreSQL string (e.g., "0/16B3748").
    pub fn format_lsn(lsn: u64) -> String {
        let high = (lsn >> 32) as u32;
        let low = (lsn & 0xFFFFFFFF) as u32;
        format!("{:X}/{:X}", high, low)
    }

    /// Parse LSN from PostgreSQL string format.
    pub fn parse_lsn(s: &str) -> Option<u64> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 {
            return None;
        }
        let high = u64::from_str_radix(parts[0], 16).ok()?;
        let low = u64::from_str_radix(parts[1], 16).ok()?;
        Some((high << 32) | low)
    }
}

impl Ord for WalSegmentInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.timeline_id, self.log_id, self.segment_id).cmp(&(
            other.timeline_id,
            other.log_id,
            other.segment_id,
        ))
    }
}

impl PartialOrd for WalSegmentInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// WAL coverage information for a backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalCoverage {
    /// Earliest WAL position available.
    pub earliest_lsn: Option<String>,
    /// Latest WAL position available.
    pub latest_lsn: Option<String>,
    /// Earliest timestamp covered (estimated from segment modification times).
    pub earliest_time: Option<DateTime<Utc>>,
    /// Latest timestamp covered (estimated from segment modification times).
    pub latest_time: Option<DateTime<Utc>>,
    /// Total number of WAL segments.
    pub segment_count: usize,
    /// Total size of WAL segments in bytes.
    pub total_size_bytes: u64,
    /// List of available timelines.
    pub timelines: Vec<u32>,
    /// Gaps in WAL coverage (missing segments).
    pub gaps: Vec<WalGap>,
}

/// Represents a gap in WAL coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalGap {
    /// Timeline where the gap exists.
    pub timeline_id: u32,
    /// First missing segment.
    pub start_segment: String,
    /// Last missing segment.
    pub end_segment: String,
    /// Number of missing segments.
    pub missing_count: usize,
}

/// A computed recovery plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPlan {
    /// Unique plan identifier.
    pub id: Uuid,
    /// When this plan was computed.
    pub computed_at: DateTime<Utc>,
    /// The recovery target.
    pub target: RecoveryTarget,
    /// The base backup to use.
    pub base_backup: BaseBackupInfo,
    /// WAL segments required for recovery (in order).
    pub wal_segments: Vec<WalSegmentInfo>,
    /// Estimated recovery time window.
    pub recovery_window: RecoveryWindow,
    /// Validation status.
    pub validation: PlanValidation,
    /// Estimated total download size (for remote backups).
    pub estimated_download_bytes: u64,
}

/// Time window for recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryWindow {
    /// Earliest recoverable point (backup start time).
    pub earliest: DateTime<Utc>,
    /// Latest recoverable point (end of WAL coverage).
    pub latest: Option<DateTime<Utc>>,
    /// Whether the target time is within this window.
    pub target_in_window: bool,
}

/// Validation result for a recovery plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanValidation {
    /// Whether the plan is valid and can be executed.
    pub is_valid: bool,
    /// Validation errors (if any).
    pub errors: Vec<String>,
    /// Validation warnings (non-fatal issues).
    pub warnings: Vec<String>,
}

impl PlanValidation {
    /// Create a valid plan validation.
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create an invalid plan validation with errors.
    pub fn invalid(errors: Vec<String>) -> Self {
        Self {
            is_valid: false,
            errors,
            warnings: Vec::new(),
        }
    }

    /// Add a warning to the validation.
    pub fn with_warning(mut self, warning: String) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Add an error and mark as invalid.
    pub fn with_error(mut self, error: String) -> Self {
        self.is_valid = false;
        self.errors.push(error);
        self
    }
}

/// Result of a PITR execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitrResult {
    /// Unique result identifier.
    pub id: Uuid,
    /// The plan that was executed.
    pub plan_id: Uuid,
    /// When execution started.
    pub started_at: DateTime<Utc>,
    /// When execution completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Execution status.
    pub status: PitrStatus,
    /// Target directory where recovery was performed.
    pub target_dir: PathBuf,
    /// Error message if failed.
    pub error_message: Option<String>,
    /// Execution details.
    pub details: PitrDetails,
}

/// Status of PITR execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PitrStatus {
    /// Recovery is in progress.
    InProgress,
    /// Recovery completed successfully.
    Completed,
    /// Recovery failed.
    Failed,
    /// Recovery was cancelled.
    Cancelled,
}

/// Detailed information about PITR execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PitrDetails {
    /// Number of WAL segments downloaded.
    pub wal_segments_downloaded: usize,
    /// Number of WAL segments applied.
    pub wal_segments_applied: usize,
    /// Total bytes downloaded.
    pub bytes_downloaded: u64,
    /// Time spent downloading (seconds).
    pub download_duration_secs: u64,
    /// Time spent applying WAL (seconds).
    pub apply_duration_secs: u64,
    /// PostgreSQL recovery mode used.
    pub recovery_mode: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_target_parse_rfc3339() {
        let target = RecoveryTarget::parse("2025-01-15T10:30:00Z").unwrap();
        assert!(matches!(target, RecoveryTarget::Time(_)));
    }

    #[test]
    fn test_recovery_target_parse_lsn() {
        let target = RecoveryTarget::parse("0/16B3748").unwrap();
        assert!(matches!(target, RecoveryTarget::Lsn(_)));
    }

    #[test]
    fn test_recovery_target_parse_latest() {
        let target = RecoveryTarget::parse("latest").unwrap();
        assert!(matches!(target, RecoveryTarget::Latest));
    }

    #[test]
    fn test_wal_segment_parse() {
        let seg = WalSegmentInfo::parse_filename(
            "000000010000000000000001",
            "/path/to/wal".to_string(),
            16 * 1024 * 1024,
            None,
            false,
        )
        .unwrap();

        assert_eq!(seg.timeline_id, 1);
        assert_eq!(seg.log_id, 0);
        assert_eq!(seg.segment_id, 1);
    }

    #[test]
    fn test_wal_segment_parse_compressed() {
        let seg = WalSegmentInfo::parse_filename(
            "000000010000000000000001.gz",
            "/path/to/wal".to_string(),
            1024 * 1024,
            None,
            true,
        )
        .unwrap();

        assert!(seg.is_compressed);
        assert_eq!(seg.timeline_id, 1);
    }

    #[test]
    fn test_lsn_format_parse_roundtrip() {
        let lsn: u64 = 0x0000000016B3748;
        let formatted = WalSegmentInfo::format_lsn(lsn);
        let parsed = WalSegmentInfo::parse_lsn(&formatted).unwrap();
        assert_eq!(lsn, parsed);
    }

    #[test]
    fn test_wal_segment_ordering() {
        let seg1 = WalSegmentInfo::parse_filename(
            "000000010000000000000001",
            "/path".to_string(),
            0,
            None,
            false,
        )
        .unwrap();
        let seg2 = WalSegmentInfo::parse_filename(
            "000000010000000000000002",
            "/path".to_string(),
            0,
            None,
            false,
        )
        .unwrap();
        let seg3 = WalSegmentInfo::parse_filename(
            "000000020000000000000001",
            "/path".to_string(),
            0,
            None,
            false,
        )
        .unwrap();

        assert!(seg1 < seg2);
        assert!(seg2 < seg3);
    }
}
