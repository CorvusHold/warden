//! Unit tests for PITR functionality.

use chrono::{Duration, Timelike, Utc};
use postgres::pitr::{PitrPlanner, RecoveryTarget, WalInventory, WalSegmentInfo};
use tempfile::TempDir;

/// Test parsing of recovery targets
mod recovery_target_tests {
    use super::*;

    #[test]
    fn test_parse_rfc3339_timestamp() {
        let target = RecoveryTarget::parse("2025-01-15T10:30:00Z").unwrap();
        assert!(target.is_time_based());

        let time = target.as_time().unwrap();
        assert_eq!(time.hour(), 10);
        assert_eq!(time.minute(), 30);
    }

    #[test]
    fn test_parse_rfc3339_with_timezone() {
        let target = RecoveryTarget::parse("2025-01-15T10:30:00+02:00").unwrap();
        assert!(target.is_time_based());
    }

    #[test]
    fn test_parse_lsn() {
        let target = RecoveryTarget::parse("0/16B3748").unwrap();
        assert!(matches!(target, RecoveryTarget::Lsn(_)));
    }

    #[test]
    fn test_parse_latest() {
        let target = RecoveryTarget::parse("latest").unwrap();
        assert!(matches!(target, RecoveryTarget::Latest));

        let target_upper = RecoveryTarget::parse("LATEST").unwrap();
        assert!(matches!(target_upper, RecoveryTarget::Latest));
    }

    #[test]
    fn test_parse_restore_point() {
        let target = RecoveryTarget::parse("my_restore_point").unwrap();
        assert!(matches!(target, RecoveryTarget::RestorePoint(_)));
    }
}

/// Test WAL segment parsing
mod wal_segment_tests {
    use super::*;

    #[test]
    fn test_parse_standard_wal_filename() {
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
        assert!(!seg.is_compressed);
        assert!(!seg.is_remote);
    }

    #[test]
    fn test_parse_compressed_wal_gz() {
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
    fn test_parse_compressed_wal_lz4() {
        let seg = WalSegmentInfo::parse_filename(
            "000000010000000000000001.lz4",
            "/path/to/wal".to_string(),
            1024 * 1024,
            None,
            true,
        )
        .unwrap();

        assert!(seg.is_compressed);
    }

    #[test]
    fn test_parse_partial_wal() {
        let seg = WalSegmentInfo::parse_filename(
            "000000010000000000000001.partial",
            "/path/to/wal".to_string(),
            1024 * 1024,
            None,
            false,
        )
        .unwrap();

        assert_eq!(seg.timeline_id, 1);
    }

    #[test]
    fn test_invalid_wal_filename() {
        // Too short
        assert!(
            WalSegmentInfo::parse_filename("00000001", "/path".to_string(), 0, None, false,)
                .is_none()
        );

        // Invalid characters
        assert!(WalSegmentInfo::parse_filename(
            "GGGGGGGGGGGGGGGGGGGGGGGG",
            "/path".to_string(),
            0,
            None,
            false,
        )
        .is_none());
    }

    #[test]
    fn test_lsn_range_calculation() {
        let seg = WalSegmentInfo::parse_filename(
            "000000010000000000000001",
            "/path".to_string(),
            16 * 1024 * 1024,
            None,
            false,
        )
        .unwrap();

        let (start, end) = seg.lsn_range();
        assert_eq!(start, 16 * 1024 * 1024); // Segment 1 starts at 16MB
        assert_eq!(end, 32 * 1024 * 1024 - 1); // Ends at 32MB - 1
    }

    #[test]
    fn test_lsn_format_and_parse() {
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
        assert!(seg1 < seg3);
    }
}

/// Test WAL inventory
mod wal_inventory_tests {
    use super::*;

    fn create_segment(timeline: u32, log: u32, seg: u32) -> WalSegmentInfo {
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
        inv.add_segment(create_segment(1, 0, 1));
        inv.add_segment(create_segment(1, 0, 2));
        inv.add_segment(create_segment(1, 0, 3));

        assert_eq!(inv.segments().len(), 3);
    }

    #[test]
    fn test_inventory_deduplication() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_segment(1, 0, 1));
        inv.add_segment(create_segment(1, 0, 1)); // Duplicate

        assert_eq!(inv.segments().len(), 1);
    }

    #[test]
    fn test_inventory_timelines() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_segment(1, 0, 1));
        inv.add_segment(create_segment(1, 0, 2));
        inv.add_segment(create_segment(2, 0, 1));
        inv.add_segment(create_segment(3, 0, 1));

        let timelines = inv.timelines();
        assert_eq!(timelines, vec![1, 2, 3]);
    }

    #[test]
    fn test_segments_for_timeline() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_segment(1, 0, 1));
        inv.add_segment(create_segment(1, 0, 2));
        inv.add_segment(create_segment(2, 0, 1));

        let timeline1_segs = inv.segments_for_timeline(1);
        assert_eq!(timeline1_segs.len(), 2);

        let timeline2_segs = inv.segments_for_timeline(2);
        assert_eq!(timeline2_segs.len(), 1);
    }

    #[test]
    fn test_coverage_calculation() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_segment(1, 0, 1));
        inv.add_segment(create_segment(1, 0, 2));
        inv.add_segment(create_segment(1, 0, 3));

        let coverage = inv.calculate_coverage();
        assert_eq!(coverage.segment_count, 3);
        assert_eq!(coverage.timelines, vec![1]);
        assert!(coverage.gaps.is_empty());
        assert_eq!(coverage.total_size_bytes, 3 * 16 * 1024 * 1024);
    }

    #[test]
    fn test_gap_detection() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_segment(1, 0, 1));
        inv.add_segment(create_segment(1, 0, 2));
        // Gap: missing segment 3
        inv.add_segment(create_segment(1, 0, 4));

        let coverage = inv.calculate_coverage();
        assert_eq!(coverage.gaps.len(), 1);
        assert_eq!(coverage.gaps[0].missing_count, 1);
        assert_eq!(coverage.gaps[0].timeline_id, 1);
    }

    #[test]
    fn test_multiple_gaps() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_segment(1, 0, 1));
        // Gap: missing 2, 3
        inv.add_segment(create_segment(1, 0, 4));
        // Gap: missing 5
        inv.add_segment(create_segment(1, 0, 6));

        let coverage = inv.calculate_coverage();
        assert_eq!(coverage.gaps.len(), 2);
    }

    #[test]
    fn test_empty_inventory_coverage() {
        let inv = WalInventory::new();
        let coverage = inv.calculate_coverage();

        assert_eq!(coverage.segment_count, 0);
        assert!(coverage.earliest_lsn.is_none());
        assert!(coverage.latest_lsn.is_none());
        assert!(coverage.timelines.is_empty());
    }

    #[test]
    fn test_covers_lsn() {
        let mut inv = WalInventory::new();
        inv.add_segment(create_segment(1, 0, 1));
        inv.add_segment(create_segment(1, 0, 2));

        // LSN within segment 1 (16MB - 32MB)
        assert!(inv.covers_lsn("0/1000000")); // 16MB

        // LSN before all segments
        assert!(!inv.covers_lsn("0/100")); // Very early
    }
}

/// Test recovery plan computation
mod recovery_plan_tests {
    use super::*;
    use std::fs;

    fn setup_test_backup_dir() -> TempDir {
        let temp_dir = TempDir::new().unwrap();

        // Create a mock backup catalog
        let catalog = serde_json::json!({
            "backups": [
                {
                    "id": "550e8400-e29b-41d4-a716-446655440000",
                    "backup_type": "Full",
                    "status": "Completed",
                    "start_time": (Utc::now() - Duration::hours(2)).to_rfc3339(),
                    "end_time": (Utc::now() - Duration::hours(1)).to_rfc3339(),
                    "wal_start": "0/1000000",
                    "wal_end": "0/2000000",
                    "size_bytes": 1024 * 1024,
                    "backup_path": temp_dir.path().join("backup1").to_string_lossy(),
                    "server_version": "15.0"
                }
            ]
        });

        fs::write(
            temp_dir.path().join("backup_catalog.json"),
            serde_json::to_string_pretty(&catalog).unwrap(),
        )
        .unwrap();

        // Create the backup directory
        fs::create_dir_all(temp_dir.path().join("backup1")).unwrap();

        temp_dir
    }

    #[test]
    fn test_planner_creation() {
        let temp_dir = TempDir::new().unwrap();
        let _planner = PitrPlanner::new(temp_dir.path().to_path_buf());
        // Just verify it doesn't panic
    }

    #[tokio::test]
    async fn test_list_recovery_options_empty() {
        let temp_dir = TempDir::new().unwrap();
        let planner = PitrPlanner::new(temp_dir.path().to_path_buf());

        let options = planner.list_recovery_options().await.unwrap();
        assert!(options.available_backups.is_empty());
        assert_eq!(options.wal_coverage.segment_count, 0);
    }
}

/// Test plan validation
mod validation_tests {
    use postgres::pitr::PlanValidation;

    #[test]
    fn test_valid_plan() {
        let validation = PlanValidation::valid();
        assert!(validation.is_valid);
        assert!(validation.errors.is_empty());
        assert!(validation.warnings.is_empty());
    }

    #[test]
    fn test_invalid_plan() {
        let validation =
            PlanValidation::invalid(vec!["Error 1".to_string(), "Error 2".to_string()]);
        assert!(!validation.is_valid);
        assert_eq!(validation.errors.len(), 2);
    }

    #[test]
    fn test_add_warning() {
        let validation = PlanValidation::valid().with_warning("Warning 1".to_string());

        assert!(validation.is_valid);
        assert_eq!(validation.warnings.len(), 1);
    }

    #[test]
    fn test_add_error() {
        let validation = PlanValidation::valid().with_error("Error 1".to_string());

        assert!(!validation.is_valid);
        assert_eq!(validation.errors.len(), 1);
    }
}
