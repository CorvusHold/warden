//! Fault injection framework for chaos testing.
//!
//! Provides a configurable way to inject faults into various system components
//! during testing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Types of faults that can be injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultType {
    /// Simulate network timeout
    NetworkTimeout,
    /// Simulate connection refused
    ConnectionRefused,
    /// Simulate high latency
    HighLatency,
    /// Simulate intermittent failures (flapping)
    Intermittent,
    /// Simulate disk full error
    DiskFull,
    /// Simulate permission denied error
    PermissionDenied,
    /// Simulate corrupted data
    DataCorruption,
    /// Simulate process crash
    ProcessCrash,
    /// Simulate partial write
    PartialWrite,
    /// Simulate S3 service unavailable
    S3Unavailable,
    /// Simulate S3 slow response
    S3SlowResponse,
    /// Simulate authentication failure
    AuthFailure,
}

impl std::fmt::Display for FaultType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FaultType::NetworkTimeout => write!(f, "network_timeout"),
            FaultType::ConnectionRefused => write!(f, "connection_refused"),
            FaultType::HighLatency => write!(f, "high_latency"),
            FaultType::Intermittent => write!(f, "intermittent"),
            FaultType::DiskFull => write!(f, "disk_full"),
            FaultType::PermissionDenied => write!(f, "permission_denied"),
            FaultType::DataCorruption => write!(f, "data_corruption"),
            FaultType::ProcessCrash => write!(f, "process_crash"),
            FaultType::PartialWrite => write!(f, "partial_write"),
            FaultType::S3Unavailable => write!(f, "s3_unavailable"),
            FaultType::S3SlowResponse => write!(f, "s3_slow_response"),
            FaultType::AuthFailure => write!(f, "auth_failure"),
        }
    }
}

/// Configuration for a specific fault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultConfig {
    /// Type of fault to inject.
    pub fault_type: FaultType,
    /// Probability of fault occurring (0.0 - 1.0).
    pub probability: f64,
    /// Duration of the fault effect (for latency, etc.).
    pub duration: Option<Duration>,
    /// Number of times to trigger before auto-disabling.
    pub trigger_count: Option<u64>,
    /// Whether the fault is currently enabled.
    pub enabled: bool,
    /// Additional parameters for the fault.
    pub params: HashMap<String, String>,
}

impl FaultConfig {
    /// Create a new fault configuration.
    pub fn new(fault_type: FaultType) -> Self {
        Self {
            fault_type,
            probability: 1.0,
            duration: None,
            trigger_count: None,
            enabled: true,
            params: HashMap::new(),
        }
    }

    /// Set the probability of the fault occurring.
    pub fn with_probability(mut self, probability: f64) -> Self {
        self.probability = probability.clamp(0.0, 1.0);
        self
    }

    /// Set the duration of the fault effect.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Set the number of times to trigger.
    pub fn with_trigger_count(mut self, count: u64) -> Self {
        self.trigger_count = Some(count);
        self
    }

    /// Add a parameter.
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// Disable the fault.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Fault injector that manages active faults.
#[derive(Debug, Clone)]
pub struct FaultInjector {
    inner: Arc<FaultInjectorInner>,
}

/// Fault configuration with per-fault trigger tracking.
#[derive(Debug)]
struct TrackedFaultConfig {
    /// The fault configuration.
    config: FaultConfig,
    /// Per-fault trigger count.
    trigger_count: AtomicU64,
}

impl TrackedFaultConfig {
    fn new(config: FaultConfig) -> Self {
        Self {
            config,
            trigger_count: AtomicU64::new(0),
        }
    }
}

#[derive(Debug)]
struct FaultInjectorInner {
    /// Active faults by component (with per-fault trigger tracking).
    faults: std::sync::RwLock<HashMap<String, Vec<TrackedFaultConfig>>>,
    /// Global enable/disable flag.
    enabled: AtomicBool,
    /// Counter for total faults triggered (global).
    trigger_count: AtomicU64,
    /// Counter for faults by type.
    type_counts: std::sync::RwLock<HashMap<FaultType, u64>>,
}

impl Default for FaultInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl FaultInjector {
    /// Create a new fault injector.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(FaultInjectorInner {
                faults: std::sync::RwLock::new(HashMap::new()),
                enabled: AtomicBool::new(true),
                trigger_count: AtomicU64::new(0),
                type_counts: std::sync::RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Register a fault for a component.
    pub fn register_fault(&self, component: impl Into<String>, config: FaultConfig) {
        if let Ok(mut faults) = self.inner.faults.write() {
            faults
                .entry(component.into())
                .or_default()
                .push(TrackedFaultConfig::new(config));
        }
    }

    /// Remove all faults for a component.
    pub fn clear_faults(&self, component: &str) {
        if let Ok(mut faults) = self.inner.faults.write() {
            faults.remove(component);
        }
    }

    /// Clear all registered faults.
    pub fn clear_all_faults(&self) {
        if let Ok(mut faults) = self.inner.faults.write() {
            faults.clear();
        }
    }

    /// Enable or disable the fault injector globally.
    pub fn set_enabled(&self, enabled: bool) {
        self.inner.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Check if a fault should be triggered for a component.
    pub fn should_trigger(&self, component: &str, fault_type: FaultType) -> Option<FaultConfig> {
        if !self.inner.enabled.load(Ordering::SeqCst) {
            return None;
        }

        let faults = self.inner.faults.read().ok()?;
        let component_faults = faults.get(component)?;

        for tracked in component_faults {
            let config = &tracked.config;
            if config.fault_type == fault_type && config.enabled {
                // Check probability
                if config.probability < 1.0 {
                    let random: f64 = rand::random();
                    if random > config.probability {
                        continue;
                    }
                }

                // Check per-fault trigger count (not global)
                if let Some(max_triggers) = config.trigger_count {
                    let current = tracked.trigger_count.load(Ordering::SeqCst);
                    if current >= max_triggers {
                        continue;
                    }
                }

                // Increment per-fault counter
                tracked.trigger_count.fetch_add(1, Ordering::SeqCst);
                // Increment global counter
                self.inner.trigger_count.fetch_add(1, Ordering::SeqCst);
                if let Ok(mut counts) = self.inner.type_counts.write() {
                    *counts.entry(fault_type).or_insert(0) += 1;
                }

                return Some(config.clone());
            }
        }

        None
    }

    /// Get the total number of faults triggered.
    pub fn total_triggers(&self) -> u64 {
        self.inner.trigger_count.load(Ordering::SeqCst)
    }

    /// Get trigger counts by fault type.
    pub fn trigger_counts_by_type(&self) -> HashMap<FaultType, u64> {
        self.inner
            .type_counts
            .read()
            .map(|counts| counts.clone())
            .unwrap_or_default()
    }

    /// Reset all counters.
    pub fn reset_counters(&self) {
        self.inner.trigger_count.store(0, Ordering::SeqCst);
        if let Ok(mut counts) = self.inner.type_counts.write() {
            counts.clear();
        }
    }
}

/// Result of a fault injection check.
#[derive(Debug, Clone)]
pub struct FaultResult {
    /// Whether a fault was triggered.
    pub triggered: bool,
    /// The fault type that was triggered.
    pub fault_type: Option<FaultType>,
    /// Error message to return.
    pub error_message: Option<String>,
    /// Delay to introduce.
    pub delay: Option<Duration>,
}

impl FaultResult {
    /// Create a result indicating no fault.
    pub fn no_fault() -> Self {
        Self {
            triggered: false,
            fault_type: None,
            error_message: None,
            delay: None,
        }
    }

    /// Create a result indicating a fault.
    pub fn fault(fault_type: FaultType, message: impl Into<String>) -> Self {
        Self {
            triggered: true,
            fault_type: Some(fault_type),
            error_message: Some(message.into()),
            delay: None,
        }
    }

    /// Create a result with a delay.
    pub fn with_delay(fault_type: FaultType, delay: Duration) -> Self {
        Self {
            triggered: true,
            fault_type: Some(fault_type),
            error_message: None,
            delay: Some(delay),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_config_builder() {
        let config = FaultConfig::new(FaultType::NetworkTimeout)
            .with_probability(0.5)
            .with_duration(Duration::from_secs(5))
            .with_trigger_count(10)
            .with_param("host", "localhost");

        assert_eq!(config.fault_type, FaultType::NetworkTimeout);
        assert_eq!(config.probability, 0.5);
        assert_eq!(config.duration, Some(Duration::from_secs(5)));
        assert_eq!(config.trigger_count, Some(10));
        assert_eq!(config.params.get("host"), Some(&"localhost".to_string()));
    }

    #[test]
    fn test_fault_injector_registration() {
        let injector = FaultInjector::new();

        injector.register_fault(
            "postgres",
            FaultConfig::new(FaultType::ConnectionRefused),
        );

        let result = injector.should_trigger("postgres", FaultType::ConnectionRefused);
        assert!(result.is_some());

        let result = injector.should_trigger("postgres", FaultType::DiskFull);
        assert!(result.is_none());
    }

    #[test]
    fn test_fault_injector_disabled() {
        let injector = FaultInjector::new();

        injector.register_fault(
            "postgres",
            FaultConfig::new(FaultType::ConnectionRefused),
        );

        injector.set_enabled(false);

        let result = injector.should_trigger("postgres", FaultType::ConnectionRefused);
        assert!(result.is_none());
    }

    #[test]
    fn test_fault_injector_counters() {
        let injector = FaultInjector::new();

        injector.register_fault(
            "postgres",
            FaultConfig::new(FaultType::ConnectionRefused),
        );

        injector.should_trigger("postgres", FaultType::ConnectionRefused);
        injector.should_trigger("postgres", FaultType::ConnectionRefused);

        assert_eq!(injector.total_triggers(), 2);

        let counts = injector.trigger_counts_by_type();
        assert_eq!(counts.get(&FaultType::ConnectionRefused), Some(&2));
    }
}
