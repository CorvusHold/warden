//! Status and observability module for PostgreSQL backup operations.
//!
//! This module provides comprehensive status information about:
//! - Backup health and history
//! - PITR coverage and recovery windows
//! - Retention policy status and storage usage
//! - Schedule status (if configured)

pub mod collector;
pub mod metrics;
pub mod types;

pub use collector::{StatusCollector, StatusCollectorConfig, StatusThresholds, StorageConfig};
pub use metrics::{Metrics, MetricsExporter};
pub use types::*;

// Re-export performance metrics types
pub use types::{
    BackupPerformanceMetrics, PitrPerformanceMetrics, 
    RetentionPerformanceMetrics, PerformanceMetrics,
};
