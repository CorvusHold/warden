//! WAL segment metadata and inventory for PITR-aware retention.

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::Path;

use super::BackupLocation;

/// Represents a single WAL segment file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalSegment {
    /// Segment file name (e.g., 000000010000000000000001)
    pub name: String,
    /// Timeline ID extracted from the name
    pub timeline: u32,
    /// Log ID (high 32 bits of LSN)
    pub log_id: u32,
    /// Segment number (low 32 bits)
    pub segment_id: u32,
    /// Size in bytes
    pub size_bytes: u64,
    /// Last modified time
    pub last_modified: Option<DateTime<Utc>>,
    /// Location of the segment
    pub location: BackupLocation,
    /// Whether this is a partial segment (.partial suffix)
    pub is_partial: bool,
    /// Whether this is a backup label or history file
    pub is_metadata: bool,
}

impl WalSegment {
    /// Parses a WAL segment from a filename
    pub fn from_filename(name: &str, size_bytes: u64, location: BackupLocation) -> Option<Self> {
        // Standard WAL segment: TTTTTTTTLLLLLLLLSSSSSSSS (24 hex chars)
        // Timeline (8) + LogID (8) + SegmentID (8)
        let base_name = name
            .trim_end_matches(".partial")
            .trim_end_matches(".gz")
            .trim_end_matches(".lz4")
            .trim_end_matches(".zst");

        let is_partial = name.contains(".partial");

        // Check for metadata files
        let is_metadata = base_name.ends_with(".backup")
            || base_name.ends_with(".history")
            || base_name.contains(".backup.");

        // Parse the segment name
        let segment_regex =
            Regex::new(r"^([0-9A-Fa-f]{8})([0-9A-Fa-f]{8})([0-9A-Fa-f]{8})").ok()?;

        if let Some(caps) = segment_regex.captures(base_name) {
            let timeline = u32::from_str_radix(&caps[1], 16).ok()?;
            let log_id = u32::from_str_radix(&caps[2], 16).ok()?;
            let segment_id = u32::from_str_radix(&caps[3], 16).ok()?;

            Some(Self {
                name: name.to_string(),
                timeline,
                log_id,
                segment_id,
                size_bytes,
                last_modified: None,
                location,
                is_partial,
                is_metadata,
            })
        } else {
            None
        }
    }

    /// Returns the LSN represented by this segment
    pub fn lsn(&self) -> u64 {
        ((self.log_id as u64) << 32) | (self.segment_id as u64)
    }

    /// Checks if this segment is needed to recover to a given LSN
    pub fn is_needed_for_lsn(&self, target_lsn: u64) -> bool {
        self.lsn() <= target_lsn
    }

    /// Checks if this segment is within a time range
    pub fn is_within_time_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
        if let Some(modified) = self.last_modified {
            modified >= start && modified <= end
        } else {
            // If no timestamp, assume it's needed (conservative)
            true
        }
    }
}

impl PartialEq for WalSegment {
    fn eq(&self, other: &Self) -> bool {
        self.timeline == other.timeline && self.lsn() == other.lsn()
    }
}

impl Eq for WalSegment {}

impl PartialOrd for WalSegment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WalSegment {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.timeline.cmp(&other.timeline) {
            Ordering::Equal => self.lsn().cmp(&other.lsn()),
            other => other,
        }
    }
}

/// Inventory of WAL segments for retention decisions
#[derive(Debug, Clone, Default)]
pub struct WalInventory {
    /// All WAL segments
    pub segments: Vec<WalSegment>,
    /// Timeline history (for timeline switches)
    pub timelines: Vec<u32>,
}

impl WalInventory {
    /// Creates a new empty inventory
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a segment to the inventory
    pub fn add_segment(&mut self, segment: WalSegment) {
        if !self.timelines.contains(&segment.timeline) {
            self.timelines.push(segment.timeline);
            self.timelines.sort();
        }
        self.segments.push(segment);
    }

    /// Sorts segments by timeline and LSN
    pub fn sort(&mut self) {
        self.segments.sort();
    }

    /// Returns the earliest segment
    pub fn earliest(&self) -> Option<&WalSegment> {
        self.segments.iter().min()
    }

    /// Returns the latest segment
    pub fn latest(&self) -> Option<&WalSegment> {
        self.segments.iter().max()
    }

    /// Returns segments needed to recover from a base backup LSN to a target time
    pub fn segments_for_recovery(
        &self,
        base_lsn: u64,
        target_time: DateTime<Utc>,
    ) -> Vec<&WalSegment> {
        self.segments
            .iter()
            .filter(|s| {
                // Include segments from base LSN onwards
                s.lsn() >= base_lsn
                    // And up to target time
                    && s.last_modified.map(|t| t <= target_time).unwrap_or(true)
            })
            .collect()
    }

    /// Returns segments within a PITR window
    pub fn segments_within_window(
        &self,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
    ) -> Vec<&WalSegment> {
        self.segments
            .iter()
            .filter(|s| s.is_within_time_range(window_start, window_end))
            .collect()
    }

    /// Returns the total size of all segments
    pub fn total_size(&self) -> u64 {
        self.segments.iter().map(|s| s.size_bytes).sum()
    }

    /// Returns segments older than a given time
    pub fn segments_older_than(&self, cutoff: DateTime<Utc>) -> Vec<&WalSegment> {
        self.segments
            .iter()
            .filter(|s| s.last_modified.map(|t| t < cutoff).unwrap_or(false))
            .collect()
    }

    /// Returns the PITR window (earliest to latest recoverable time)
    pub fn pitr_window(&self) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let earliest = self.segments.iter().filter_map(|s| s.last_modified).min()?;
        let latest = self.segments.iter().filter_map(|s| s.last_modified).max()?;
        Some((earliest, latest))
    }

    /// Scans a local directory for WAL segments
    pub fn scan_local_directory(path: &Path) -> std::io::Result<Self> {
        let mut inventory = Self::new();

        if !path.exists() {
            return Ok(inventory);
        }

        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let metadata = entry.metadata()?;
                let size = metadata.len();
                let modified = metadata.modified().ok().map(DateTime::<Utc>::from);

                if let Some(mut segment) = WalSegment::from_filename(
                    filename,
                    size,
                    BackupLocation::Local(path.to_string_lossy().to_string()),
                ) {
                    segment.last_modified = modified;
                    inventory.add_segment(segment);
                }
            }
        }

        inventory.sort();
        Ok(inventory)
    }
}

/// Parses an LSN string (e.g., "0/1234567") into a u64
pub fn parse_lsn(lsn: &str) -> Option<u64> {
    let parts: Vec<&str> = lsn.split('/').collect();
    if parts.len() != 2 {
        return None;
    }

    let high = u64::from_str_radix(parts[0], 16).ok()?;
    let low = u64::from_str_radix(parts[1], 16).ok()?;

    Some((high << 32) | low)
}

/// Formats a u64 LSN as a string (e.g., "0/1234567")
#[allow(dead_code)] // Public API for LSN formatting
pub fn format_lsn(lsn: u64) -> String {
    let high = lsn >> 32;
    let low = lsn & 0xFFFFFFFF;
    format!("{:X}/{:X}", high, low)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wal_segment_name() {
        let segment = WalSegment::from_filename(
            "000000010000000000000001",
            16 * 1024 * 1024,
            BackupLocation::Local("/wal/000000010000000000000001".to_string()),
        );

        assert!(segment.is_some());
        let segment = segment.unwrap();
        assert_eq!(segment.timeline, 1);
        assert_eq!(segment.log_id, 0);
        assert_eq!(segment.segment_id, 1);
        assert!(!segment.is_partial);
        assert!(!segment.is_metadata);
    }

    #[test]
    fn test_parse_partial_segment() {
        let segment = WalSegment::from_filename(
            "000000010000000000000002.partial",
            8 * 1024 * 1024,
            BackupLocation::Local("/wal/000000010000000000000002.partial".to_string()),
        );

        assert!(segment.is_some());
        let segment = segment.unwrap();
        assert!(segment.is_partial);
    }

    #[test]
    fn test_parse_compressed_segment() {
        let segment = WalSegment::from_filename(
            "000000010000000000000003.gz",
            4 * 1024 * 1024,
            BackupLocation::Local("/wal/000000010000000000000003.gz".to_string()),
        );

        assert!(segment.is_some());
        let segment = segment.unwrap();
        assert_eq!(segment.segment_id, 3);
    }

    #[test]
    fn test_parse_backup_label() {
        let segment = WalSegment::from_filename(
            "000000010000000000000001.00000028.backup",
            100,
            BackupLocation::Local("/wal/000000010000000000000001.00000028.backup".to_string()),
        );

        assert!(segment.is_some());
        let segment = segment.unwrap();
        assert!(segment.is_metadata);
    }

    #[test]
    fn test_segment_ordering() {
        let seg1 = WalSegment::from_filename(
            "000000010000000000000001",
            16 * 1024 * 1024,
            BackupLocation::Local("/wal/1".to_string()),
        )
        .unwrap();

        let seg2 = WalSegment::from_filename(
            "000000010000000000000002",
            16 * 1024 * 1024,
            BackupLocation::Local("/wal/2".to_string()),
        )
        .unwrap();

        let seg3 = WalSegment::from_filename(
            "000000020000000000000001",
            16 * 1024 * 1024,
            BackupLocation::Local("/wal/3".to_string()),
        )
        .unwrap();

        assert!(seg1 < seg2);
        assert!(seg2 < seg3);
        assert!(seg1 < seg3);
    }

    #[test]
    fn test_parse_lsn() {
        assert_eq!(parse_lsn("0/1234567"), Some(0x1234567));
        assert_eq!(parse_lsn("1/0"), Some(0x100000000));
        assert_eq!(parse_lsn("invalid"), None);
    }

    #[test]
    fn test_format_lsn() {
        assert_eq!(format_lsn(0x1234567), "0/1234567");
        assert_eq!(format_lsn(0x100000000), "1/0");
    }

    #[test]
    fn test_inventory_operations() {
        let mut inventory = WalInventory::new();

        let seg1 = WalSegment::from_filename(
            "000000010000000000000001",
            16 * 1024 * 1024,
            BackupLocation::Local("/wal/1".to_string()),
        )
        .unwrap();

        let seg2 = WalSegment::from_filename(
            "000000010000000000000002",
            16 * 1024 * 1024,
            BackupLocation::Local("/wal/2".to_string()),
        )
        .unwrap();

        inventory.add_segment(seg1);
        inventory.add_segment(seg2);
        inventory.sort();

        assert_eq!(inventory.segments.len(), 2);
        assert_eq!(inventory.earliest().unwrap().segment_id, 1);
        assert_eq!(inventory.latest().unwrap().segment_id, 2);
        assert_eq!(inventory.total_size(), 32 * 1024 * 1024);
    }
}
