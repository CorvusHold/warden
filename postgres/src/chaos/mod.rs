//! Chaos testing utilities for simulating failure scenarios.
//!
//! This module provides utilities for testing Warden's resilience under various
//! failure conditions including:
//! - PostgreSQL crashes and connection failures
//! - S3/MinIO outages and high latency
//! - Disk full and permission errors
//! - Network partitions and timeouts
//!
//! These utilities are designed to be used in integration tests and chaos testing
//! scenarios to verify that Warden handles failures gracefully.

pub mod fault_injection;
pub mod scenarios;
pub mod simulators;

pub use fault_injection::{FaultConfig, FaultInjector, FaultType};
pub use scenarios::{ChaosScenario, ScenarioResult};
pub use simulators::{
    DiskSimulator, NetworkSimulator, PostgresSimulator, StorageSimulator,
};
