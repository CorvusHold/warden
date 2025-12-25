//! WAL segment discovery and inventory management.

use chrono::{DateTime, Utc};
use log::{debug, info};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::PostgresError;

use super::types::{WalCoverage, WalGap, WalSegmentInfo};

/// WAL inventory manager for discovering and analyzing WAL segments.
pub struct WalInventory {
    /// All discovered WAL segments, keyed by (timeline, log, segment).
    segments: BTreeMap<(u32, u32, u32), WalSegmentInfo>,
}

impl WalInventory {
    /// Create a new empty WAL inventory.
    pub fn new() -> Self {
        Self {
            segments: BTreeMap::new(),
        }
    }

    /// Add a WAL segment to the inventory.
    pub fn add_segment(&mut self, segment: WalSegmentInfo) {
        let key = (segment.timeline_id, segment.log_id, segment.segment_id);
        self.segments.insert(key, segment);
    }

    /// Discover WAL segments from a local directory.
    pub fn discover_local(&mut self, wal_dir: &Path) -> Result<usize, PostgresError> {
        if !wal_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        let entries = std::fs::read_dir(wal_dir).map_err(PostgresError::Io)?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            let metadata = entry.metadata().map_err(PostgresError::Io)?;
            let last_modified = metadata.modified().ok().map(DateTime::<Utc>::from);

            if let Some(segment) = WalSegmentInfo::parse_filename(
                filename,
                path.to_string_lossy().to_string(),
                metadata.len(),
                last_modified,
                false,
            ) {
                debug!("Discovered local WAL segment: {}", filename);
                self.add_segment(segment);
                count += 1;
            }
        }

        info!("Discovered {} local WAL segments in {:?}", count, wal_dir);
        Ok(count)
    }

    /// Add WAL segments from remote storage listing.
    pub fn add_remote_segments(&mut self, objects: Vec<RemoteWalObject>) -> usize {
        let mut count = 0;
        for obj in objects {
            if let Some(segment) = WalSegmentInfo::parse_filename(
                &obj.filename,
                obj.key,
                obj.size,
                obj.last_modified,
                true,
            ) {
                debug!("Added remote WAL segment: {}", obj.filename);
                self.add_segment(segment);
                count += 1;
            }
        }
        info!("Added {} remote WAL segments", count);
        count
    }

    /// Get all segments in order.
    pub fn segments(&self) -> Vec<&WalSegmentInfo> {
        self.segments.values().collect()
    }

    /// Get segments for a specific timeline.
    pub fn segments_for_timeline(&self, timeline_id: u32) -> Vec<&WalSegmentInfo> {
        self.segments
            .values()
            .filter(|s| s.timeline_id == timeline_id)
            .collect()
    }

    /// Get all available timelines.
    pub fn timelines(&self) -> Vec<u32> {
        let mut timelines: Vec<u32> = self
            .segments
            .keys()
            .map(|(t, _, _)| *t)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        timelines.sort();
        timelines
    }

    /// Calculate WAL coverage information.
    pub fn calculate_coverage(&self) -> WalCoverage {
        if self.segments.is_empty() {
            return WalCoverage {
                earliest_lsn: None,
                latest_lsn: None,
                earliest_time: None,
                latest_time: None,
                segment_count: 0,
                total_size_bytes: 0,
                timelines: Vec::new(),
                gaps: Vec::new(),
            };
        }

        let segments: Vec<_> = self.segments.values().collect();
        let timelines = self.timelines();

        // Find earliest and latest segments
        let first = segments.first().unwrap();
        let last = segments.last().unwrap();

        let (earliest_lsn_val, _) = first.lsn_range();
        let (_, latest_lsn_val) = last.lsn_range();

        // Find time range from modification times
        let earliest_time = segments.iter().filter_map(|s| s.last_modified).min();
        let latest_time = segments.iter().filter_map(|s| s.last_modified).max();

        // Calculate total size
        let total_size_bytes: u64 = segments.iter().map(|s| s.size_bytes).sum();

        // Find gaps in coverage
        let gaps = self.find_gaps();

        WalCoverage {
            earliest_lsn: Some(WalSegmentInfo::format_lsn(earliest_lsn_val)),
            latest_lsn: Some(WalSegmentInfo::format_lsn(latest_lsn_val)),
            earliest_time,
            latest_time,
            segment_count: segments.len(),
            total_size_bytes,
            timelines,
            gaps,
        }
    }

    /// Find gaps in WAL segment coverage.
    fn find_gaps(&self) -> Vec<WalGap> {
        let mut gaps = Vec::new();

        for timeline in self.timelines() {
            let timeline_segments: Vec<_> = self
                .segments
                .iter()
                .filter(|((t, _, _), _)| *t == timeline)
                .collect();

            if timeline_segments.len() < 2 {
                continue;
            }

            let mut prev_key: Option<(u32, u32, u32)> = None;
            for (key, _seg) in &timeline_segments {
                let (t, log, seg) = **key;

                if let Some((_, prev_log, prev_seg)) = prev_key {
                    // Check for gap
                    let expected_next = if prev_seg == 0xFF {
                        (prev_log + 1, 0)
                    } else {
                        (prev_log, prev_seg + 1)
                    };

                    if (log, seg) != expected_next {
                        let missing_count =
                            self.count_missing_segments(timeline, prev_log, prev_seg, log, seg);

                        if missing_count > 0 {
                            gaps.push(WalGap {
                                timeline_id: timeline,
                                start_segment: format!(
                                    "{:08X}{:08X}{:08X}",
                                    timeline, expected_next.0, expected_next.1
                                ),
                                end_segment: format!(
                                    "{:08X}{:08X}{:08X}",
                                    timeline,
                                    if seg == 0 { log - 1 } else { log },
                                    if seg == 0 { 0xFF } else { seg - 1 }
                                ),
                                missing_count,
                            });
                        }
                    }
                }
                prev_key = Some((t, log, seg));
            }
        }

        gaps
    }

    /// Count missing segments between two positions.
    fn count_missing_segments(
        &self,
        _timeline: u32,
        start_log: u32,
        start_seg: u32,
        end_log: u32,
        end_seg: u32,
    ) -> usize {
        // Simplified calculation - assumes 256 segments per log
        let start_pos = (start_log as usize) * 256 + (start_seg as usize);
        let end_pos = (end_log as usize) * 256 + (end_seg as usize);

        if end_pos > start_pos + 1 {
            end_pos - start_pos - 1
        } else {
            0
        }
    }

    /// Get segments required to recover from a starting LSN to a target.
    /// Returns segments in order, or an error if there are gaps.
    pub fn get_segments_for_recovery(
        &self,
        start_lsn: &str,
        target_lsn: Option<&str>,
        target_time: Option<DateTime<Utc>>,
    ) -> Result<Vec<WalSegmentInfo>, PostgresError> {
        let start = WalSegmentInfo::parse_lsn(start_lsn)
            .ok_or_else(|| PostgresError::WalError(format!("Invalid start LSN: {}", start_lsn)))?;

        // Determine the end point
        let end =
            if let Some(lsn) = target_lsn {
                Some(WalSegmentInfo::parse_lsn(lsn).ok_or_else(|| {
                    PostgresError::WalError(format!("Invalid target LSN: {}", lsn))
                })?)
            } else {
                None
            };

        let mut result = Vec::new();
        let mut found_start = false;

        for segment in self.segments.values() {
            let (_seg_start, seg_end) = segment.lsn_range();

            // Check if this segment contains or is after the start LSN
            if seg_end >= start {
                found_start = true;
            }

            if !found_start {
                continue;
            }

            // Add this segment
            result.push(segment.clone());

            // Check if we've reached the target
            if let Some(end_lsn) = end {
                if seg_end >= end_lsn {
                    break;
                }
            }

            // Check if we've reached the target time
            if let Some(target) = target_time {
                if let Some(modified) = segment.last_modified {
                    if modified >= target {
                        // Include a few more segments to be safe
                        // (WAL modification time is approximate)
                        break;
                    }
                }
            }
        }

        if result.is_empty() {
            return Err(PostgresError::WalError(format!(
                "No WAL segments found starting from LSN {}",
                start_lsn
            )));
        }

        // Check for gaps in the result
        self.validate_segment_continuity(&result)?;

        Ok(result)
    }

    /// Validate that a sequence of segments is continuous (no gaps).
    fn validate_segment_continuity(
        &self,
        segments: &[WalSegmentInfo],
    ) -> Result<(), PostgresError> {
        if segments.len() < 2 {
            return Ok(());
        }

        for i in 1..segments.len() {
            let prev = &segments[i - 1];
            let curr = &segments[i];

            // Check timeline continuity (allow timeline switches)
            if prev.timeline_id != curr.timeline_id {
                // Timeline switch is allowed
                continue;
            }

            // Check segment continuity
            let expected_next = if prev.segment_id == 0xFF {
                (prev.log_id + 1, 0)
            } else {
                (prev.log_id, prev.segment_id + 1)
            };

            if (curr.log_id, curr.segment_id) != expected_next {
                return Err(PostgresError::WalError(format!(
                    "Gap in WAL segments: {} -> {} (expected {:08X}{:08X}{:08X})",
                    prev.filename,
                    curr.filename,
                    curr.timeline_id,
                    expected_next.0,
                    expected_next.1
                )));
            }
        }

        Ok(())
    }

    /// Check if WAL coverage includes a specific time.
    pub fn covers_time(&self, target_time: DateTime<Utc>) -> bool {
        let coverage = self.calculate_coverage();

        match (coverage.earliest_time, coverage.latest_time) {
            (Some(earliest), Some(latest)) => target_time >= earliest && target_time <= latest,
            _ => false,
        }
    }

    /// Check if WAL coverage includes a specific LSN.
    pub fn covers_lsn(&self, target_lsn: &str) -> bool {
        let target = match WalSegmentInfo::parse_lsn(target_lsn) {
            Some(lsn) => lsn,
            None => return false,
        };

        for segment in self.segments.values() {
            let (start, end) = segment.lsn_range();
            if target >= start && target <= end {
                return true;
            }
        }

        false
    }
}

impl Default for WalInventory {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a WAL object from remote storage.
#[derive(Debug, Clone)]
pub struct RemoteWalObject {
    /// Object key in storage.
    pub key: String,
    /// Filename extracted from key.
    pub filename: String,
    /// Size in bytes.
    pub size: u64,
    /// Last modified time.
    pub last_modified: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_segment(timeline: u32, log: u32, seg: u32) -> WalSegmentInfo {
        let filename = format!("{:08X}{:08X}{:08X}", timeline, log, seg);
        WalSegmentInfo::parse_filename(
            &filename,
            "/test".to_string(),
            16 * 1024 * 1024,
            None,
            false,
        )
        .unwrap()
    }

    #[test]
    fn test_inventory_add_and_list() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_test_segment(1, 0, 1));
        inv.add_segment(create_test_segment(1, 0, 2));
        inv.add_segment(create_test_segment(1, 0, 3));

        assert_eq!(inv.segments().len(), 3);
    }

    #[test]
    fn test_inventory_timelines() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_test_segment(1, 0, 1));
        inv.add_segment(create_test_segment(2, 0, 1));

        let timelines = inv.timelines();
        assert_eq!(timelines, vec![1, 2]);
    }

    #[test]
    fn test_coverage_calculation() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_test_segment(1, 0, 1));
        inv.add_segment(create_test_segment(1, 0, 2));
        inv.add_segment(create_test_segment(1, 0, 3));

        let coverage = inv.calculate_coverage();
        assert_eq!(coverage.segment_count, 3);
        assert_eq!(coverage.timelines, vec![1]);
        assert!(coverage.gaps.is_empty());
    }

    #[test]
    fn test_gap_detection() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_test_segment(1, 0, 1));
        inv.add_segment(create_test_segment(1, 0, 2));
        // Gap: missing segment 3
        inv.add_segment(create_test_segment(1, 0, 4));

        let coverage = inv.calculate_coverage();
        assert_eq!(coverage.gaps.len(), 1);
        assert_eq!(coverage.gaps[0].missing_count, 1);
    }

    #[test]
    fn test_segment_continuity_validation() {
        let mut inv = WalInventory::new();
        let seg1 = create_test_segment(1, 0, 1);
        let seg2 = create_test_segment(1, 0, 2);
        let seg3 = create_test_segment(1, 0, 3);

        inv.add_segment(seg1.clone());
        inv.add_segment(seg2.clone());
        inv.add_segment(seg3.clone());

        // Continuous sequence should pass
        let result = inv.validate_segment_continuity(&[seg1, seg2, seg3]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_segment_continuity_with_gap() {
        let inv = WalInventory::new();
        let seg1 = create_test_segment(1, 0, 1);
        let seg3 = create_test_segment(1, 0, 3); // Gap: missing segment 2

        let result = inv.validate_segment_continuity(&[seg1, seg3]);
        assert!(result.is_err());
    }
}
