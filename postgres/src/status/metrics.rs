//! Metrics exporter for observability.
//!
//! Provides Prometheus-compatible metrics in text format and optional HTTP endpoint.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

use super::types::{MetricGauges, MetricLabels, OperationCounters};

/// Metrics registry for tracking backup operations.
#[derive(Debug, Clone)]
pub struct Metrics {
    inner: Arc<RwLock<MetricsInner>>,
}

#[derive(Debug, Default)]
struct MetricsInner {
    /// Operation counters
    counters: OperationCounters,
    /// Gauge values
    gauges: MetricGauges,
    /// Last update time
    last_updated: Option<DateTime<Utc>>,
    /// Labels for all metrics
    labels: MetricLabels,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

impl Metrics {
    /// Create a new metrics registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MetricsInner::default())),
        }
    }

    /// Create metrics with labels.
    pub fn with_labels(labels: MetricLabels) -> Self {
        let metrics = Self::new();
        if let Ok(mut inner) = metrics.inner.write() {
            inner.labels = labels;
        }
        metrics
    }

    /// Set a label.
    pub fn set_label(&self, key: impl Into<String>, value: impl Into<String>) {
        if let Ok(mut inner) = self.inner.write() {
            inner.labels.insert(key.into(), value.into());
        }
    }

    /// Record a successful backup.
    pub fn record_backup_success(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.backups_successful += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Record a failed backup.
    pub fn record_backup_failure(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.backups_failed += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Record a successful restore.
    pub fn record_restore_success(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.restores_successful += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Record a failed restore.
    pub fn record_restore_failure(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.restores_failed += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Record a successful PITR operation.
    pub fn record_pitr_success(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.pitr_successful += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Record a failed PITR operation.
    pub fn record_pitr_failure(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.pitr_failed += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Record a successful purge operation.
    pub fn record_purge_success(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.purges_successful += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Record a failed purge operation.
    pub fn record_purge_failure(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.counters.purges_failed += 1;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Update gauge values.
    pub fn update_gauges(&self, gauges: MetricGauges) {
        if let Ok(mut inner) = self.inner.write() {
            inner.gauges = gauges;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Set latest backup age gauge.
    pub fn set_latest_backup_age(&self, age_seconds: f64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.gauges.latest_backup_age_seconds = Some(age_seconds);
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Set PITR window gauge.
    pub fn set_pitr_window(&self, window_seconds: f64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.gauges.pitr_window_seconds = Some(window_seconds);
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Set backup storage gauge.
    pub fn set_backup_storage(&self, bytes: u64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.gauges.backup_storage_bytes = bytes;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Set WAL storage gauge.
    pub fn set_wal_storage(&self, bytes: u64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.gauges.wal_storage_bytes = bytes;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Set available backups gauge.
    pub fn set_available_backups(&self, count: u64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.gauges.available_backups = count;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Set WAL segments gauge.
    pub fn set_wal_segments(&self, count: u64) {
        if let Ok(mut inner) = self.inner.write() {
            inner.gauges.wal_segments = count;
            inner.last_updated = Some(Utc::now());
        }
    }

    /// Get current counters.
    pub fn counters(&self) -> OperationCounters {
        self.inner
            .read()
            .map(|inner| inner.counters.clone())
            .unwrap_or_default()
    }

    /// Get current gauges.
    pub fn gauges(&self) -> MetricGauges {
        self.inner
            .read()
            .map(|inner| inner.gauges.clone())
            .unwrap_or_default()
    }

    /// Export metrics in Prometheus text format.
    pub fn export_prometheus(&self) -> String {
        let inner = match self.inner.read() {
            Ok(inner) => inner,
            Err(_) => return String::new(),
        };

        let labels_str = format_labels(&inner.labels);
        let mut output = String::new();

        // Counters
        output.push_str("# HELP warden_backups_total Total number of backup operations\n");
        output.push_str("# TYPE warden_backups_total counter\n");
        output.push_str(&format!(
            "warden_backups_total{{status=\"success\"{}}} {}\n",
            labels_str, inner.counters.backups_successful
        ));
        output.push_str(&format!(
            "warden_backups_total{{status=\"failure\"{}}} {}\n",
            labels_str, inner.counters.backups_failed
        ));

        output.push_str("# HELP warden_restores_total Total number of restore operations\n");
        output.push_str("# TYPE warden_restores_total counter\n");
        output.push_str(&format!(
            "warden_restores_total{{status=\"success\"{}}} {}\n",
            labels_str, inner.counters.restores_successful
        ));
        output.push_str(&format!(
            "warden_restores_total{{status=\"failure\"{}}} {}\n",
            labels_str, inner.counters.restores_failed
        ));

        output.push_str("# HELP warden_pitr_total Total number of PITR operations\n");
        output.push_str("# TYPE warden_pitr_total counter\n");
        output.push_str(&format!(
            "warden_pitr_total{{status=\"success\"{}}} {}\n",
            labels_str, inner.counters.pitr_successful
        ));
        output.push_str(&format!(
            "warden_pitr_total{{status=\"failure\"{}}} {}\n",
            labels_str, inner.counters.pitr_failed
        ));

        output.push_str("# HELP warden_purges_total Total number of purge operations\n");
        output.push_str("# TYPE warden_purges_total counter\n");
        output.push_str(&format!(
            "warden_purges_total{{status=\"success\"{}}} {}\n",
            labels_str, inner.counters.purges_successful
        ));
        output.push_str(&format!(
            "warden_purges_total{{status=\"failure\"{}}} {}\n",
            labels_str, inner.counters.purges_failed
        ));

        // Gauges
        output.push_str(
            "# HELP warden_latest_backup_age_seconds Age of the most recent successful backup\n",
        );
        output.push_str("# TYPE warden_latest_backup_age_seconds gauge\n");
        if let Some(age) = inner.gauges.latest_backup_age_seconds {
            output.push_str(&format!(
                "warden_latest_backup_age_seconds{{{}}} {:.2}\n",
                labels_str.trim_start_matches(',').trim(),
                age
            ));
        }

        output.push_str("# HELP warden_pitr_window_seconds Size of the PITR recovery window\n");
        output.push_str("# TYPE warden_pitr_window_seconds gauge\n");
        if let Some(window) = inner.gauges.pitr_window_seconds {
            output.push_str(&format!(
                "warden_pitr_window_seconds{{{}}} {:.2}\n",
                labels_str.trim_start_matches(',').trim(),
                window
            ));
        }

        output.push_str("# HELP warden_backup_storage_bytes Total backup storage used\n");
        output.push_str("# TYPE warden_backup_storage_bytes gauge\n");
        output.push_str(&format!(
            "warden_backup_storage_bytes{{{}}} {}\n",
            labels_str.trim_start_matches(',').trim(),
            inner.gauges.backup_storage_bytes
        ));

        output.push_str("# HELP warden_wal_storage_bytes Total WAL storage used\n");
        output.push_str("# TYPE warden_wal_storage_bytes gauge\n");
        output.push_str(&format!(
            "warden_wal_storage_bytes{{{}}} {}\n",
            labels_str.trim_start_matches(',').trim(),
            inner.gauges.wal_storage_bytes
        ));

        output.push_str("# HELP warden_available_backups Number of available backups\n");
        output.push_str("# TYPE warden_available_backups gauge\n");
        output.push_str(&format!(
            "warden_available_backups{{{}}} {}\n",
            labels_str.trim_start_matches(',').trim(),
            inner.gauges.available_backups
        ));

        output.push_str("# HELP warden_wal_segments Number of WAL segments\n");
        output.push_str("# TYPE warden_wal_segments gauge\n");
        output.push_str(&format!(
            "warden_wal_segments{{{}}} {}\n",
            labels_str.trim_start_matches(',').trim(),
            inner.gauges.wal_segments
        ));

        // Encryption metrics
        output.push_str("# HELP warden_encrypted_backups Number of encrypted backups\n");
        output.push_str("# TYPE warden_encrypted_backups gauge\n");
        output.push_str(&format!(
            "warden_encrypted_backups{{{}}} {}\n",
            labels_str.trim_start_matches(',').trim(),
            inner.gauges.encrypted_backups
        ));

        output.push_str("# HELP warden_unencrypted_backups Number of unencrypted backups\n");
        output.push_str("# TYPE warden_unencrypted_backups gauge\n");
        output.push_str(&format!(
            "warden_unencrypted_backups{{{}}} {}\n",
            labels_str.trim_start_matches(',').trim(),
            inner.gauges.unencrypted_backups
        ));

        output
    }

    /// Export metrics as JSON.
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        let inner = self.inner.read().unwrap();
        let export = MetricsExport {
            timestamp: Utc::now(),
            labels: inner.labels.clone(),
            counters: inner.counters.clone(),
            gauges: inner.gauges.clone(),
        };
        serde_json::to_string_pretty(&export)
    }
}

/// Format labels for Prometheus output.
fn format_labels(labels: &MetricLabels) -> String {
    if labels.is_empty() {
        return String::new();
    }

    let parts: Vec<String> = labels
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, v))
        .collect();

    format!(",{}", parts.join(","))
}

/// Metrics export structure for JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsExport {
    pub timestamp: DateTime<Utc>,
    pub labels: MetricLabels,
    pub counters: OperationCounters,
    pub gauges: MetricGauges,
}

/// Metrics exporter that can write to file or serve via HTTP.
pub struct MetricsExporter {
    metrics: Metrics,
    output_path: Option<std::path::PathBuf>,
}

impl MetricsExporter {
    /// Create a new exporter.
    pub fn new(metrics: Metrics) -> Self {
        Self {
            metrics,
            output_path: None,
        }
    }

    /// Set output file path for text file export.
    pub fn with_output_path(mut self, path: std::path::PathBuf) -> Self {
        self.output_path = Some(path);
        self
    }

    /// Export metrics to the configured output.
    pub fn export(&self) -> Result<(), std::io::Error> {
        let prometheus_output = self.metrics.export_prometheus();

        if let Some(path) = &self.output_path {
            std::fs::write(path, &prometheus_output)?;
        }

        Ok(())
    }

    /// Get Prometheus-format metrics as string.
    pub fn prometheus_output(&self) -> String {
        self.metrics.export_prometheus()
    }

    /// Get JSON-format metrics as string.
    pub fn json_output(&self) -> Result<String, serde_json::Error> {
        self.metrics.export_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_metrics_counters() {
        let metrics = Metrics::new();

        metrics.record_backup_success();
        metrics.record_backup_success();
        metrics.record_backup_failure();

        let counters = metrics.counters();
        assert_eq!(counters.backups_successful, 2);
        assert_eq!(counters.backups_failed, 1);
    }

    #[test]
    fn test_metrics_gauges() {
        let metrics = Metrics::new();

        metrics.set_latest_backup_age(3600.0);
        metrics.set_pitr_window(86400.0);
        metrics.set_available_backups(5);

        let gauges = metrics.gauges();
        assert_eq!(gauges.latest_backup_age_seconds, Some(3600.0));
        assert_eq!(gauges.pitr_window_seconds, Some(86400.0));
        assert_eq!(gauges.available_backups, 5);
    }

    #[test]
    fn test_prometheus_export() {
        let mut labels = HashMap::new();
        labels.insert("database".to_string(), "mydb".to_string());
        labels.insert("host".to_string(), "localhost".to_string());

        let metrics = Metrics::with_labels(labels);
        metrics.record_backup_success();
        metrics.set_latest_backup_age(3600.0);

        let output = metrics.export_prometheus();
        assert!(output.contains("warden_backups_total"));
        assert!(output.contains("warden_latest_backup_age_seconds"));
        assert!(output.contains("database=\"mydb\""));
    }

    #[test]
    fn test_json_export() {
        let metrics = Metrics::new();
        metrics.record_backup_success();

        let json = metrics.export_json().unwrap();
        assert!(json.contains("backups_successful"));
    }

    #[test]
    fn test_format_labels() {
        let mut labels = HashMap::new();
        labels.insert("db".to_string(), "test".to_string());

        let formatted = format_labels(&labels);
        assert!(formatted.contains("db=\"test\""));
    }
}
