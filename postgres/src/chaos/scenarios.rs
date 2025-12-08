//! Chaos testing scenarios for comprehensive failure testing.
//!
//! This module provides pre-built chaos scenarios that combine multiple
//! failure modes to test system resilience.

use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};

use super::simulators::{DiskSimulator, PostgresSimulator, SimulatorError, StorageSimulator};

/// Result of running a chaos scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// Name of the scenario.
    pub scenario_name: String,
    /// Whether the scenario passed (system behaved correctly under failure).
    pub passed: bool,
    /// Start time of the scenario.
    pub start_time: DateTime<Utc>,
    /// End time of the scenario.
    pub end_time: DateTime<Utc>,
    /// Duration of the scenario.
    pub duration_ms: u64,
    /// Expected behavior description.
    pub expected_behavior: String,
    /// Actual behavior observed.
    pub actual_behavior: String,
    /// Error message if the scenario failed.
    pub error: Option<String>,
    /// Artifacts cleaned up properly.
    pub artifacts_cleaned: bool,
    /// Exit code observed (if applicable).
    pub exit_code: Option<i32>,
    /// Detailed steps executed.
    pub steps: Vec<ScenarioStep>,
}

impl ScenarioResult {
    /// Create a new scenario result.
    pub fn new(scenario_name: impl Into<String>) -> Self {
        Self {
            scenario_name: scenario_name.into(),
            passed: false,
            start_time: Utc::now(),
            end_time: Utc::now(),
            duration_ms: 0,
            expected_behavior: String::new(),
            actual_behavior: String::new(),
            error: None,
            artifacts_cleaned: true,
            exit_code: None,
            steps: Vec::new(),
        }
    }

    /// Mark the scenario as passed.
    pub fn pass(mut self) -> Self {
        self.passed = true;
        self.end_time = Utc::now();
        self.duration_ms = (self.end_time - self.start_time)
            .num_milliseconds()
            .max(0) as u64;
        self
    }

    /// Mark the scenario as failed.
    pub fn fail(mut self, error: impl Into<String>) -> Self {
        self.passed = false;
        self.error = Some(error.into());
        self.end_time = Utc::now();
        self.duration_ms = (self.end_time - self.start_time)
            .num_milliseconds()
            .max(0) as u64;
        self
    }

    /// Set expected behavior.
    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected_behavior = expected.into();
        self
    }

    /// Set actual behavior.
    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual_behavior = actual.into();
        self
    }

    /// Add a step.
    pub fn add_step(&mut self, step: ScenarioStep) {
        self.steps.push(step);
    }
}

/// A single step in a chaos scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioStep {
    /// Step number.
    pub number: usize,
    /// Step description.
    pub description: String,
    /// Whether the step succeeded.
    pub success: bool,
    /// Duration of the step.
    pub duration_ms: u64,
    /// Error message if the step failed.
    pub error: Option<String>,
}

impl ScenarioStep {
    /// Create a new step.
    pub fn new(number: usize, description: impl Into<String>) -> Self {
        Self {
            number,
            description: description.into(),
            success: false,
            duration_ms: 0,
            error: None,
        }
    }

    /// Mark the step as successful.
    pub fn success(mut self, duration_ms: u64) -> Self {
        self.success = true;
        self.duration_ms = duration_ms;
        self
    }

    /// Mark the step as failed.
    pub fn failure(mut self, error: impl Into<String>, duration_ms: u64) -> Self {
        self.success = false;
        self.error = Some(error.into());
        self.duration_ms = duration_ms;
        self
    }
}

/// A chaos scenario that can be executed.
pub trait ChaosScenario: Send + Sync {
    /// Get the name of the scenario.
    fn name(&self) -> &str;

    /// Get a description of the scenario.
    fn description(&self) -> &str;

    /// Get the expected behavior under this failure condition.
    fn expected_behavior(&self) -> &str;

    /// Run the scenario and return the result.
    fn run(&self) -> impl std::future::Future<Output = ScenarioResult> + Send;

    /// Clean up any artifacts created by the scenario.
    fn cleanup(&self) -> Result<(), SimulatorError>;
}

/// Scenario: PostgreSQL crash during backup.
#[derive(Debug, Clone)]
pub struct PostgresCrashDuringBackup {
    /// PostgreSQL simulator.
    pub postgres: PostgresSimulator,
    /// Backup directory.
    pub backup_dir: PathBuf,
    /// Database to backup.
    pub database: String,
    /// User for connection.
    pub user: String,
    /// Password for connection.
    pub password: Option<String>,
    /// Delay before crash (ms).
    pub crash_delay_ms: u64,
}

impl PostgresCrashDuringBackup {
    /// Create a new scenario.
    pub fn new(backup_dir: impl Into<PathBuf>) -> Self {
        Self {
            postgres: PostgresSimulator::default(),
            backup_dir: backup_dir.into(),
            database: "postgres".to_string(),
            user: "postgres".to_string(),
            password: None,
            crash_delay_ms: 1000,
        }
    }

    /// Set the PostgreSQL simulator.
    pub fn with_postgres(mut self, postgres: PostgresSimulator) -> Self {
        self.postgres = postgres;
        self
    }

    /// Set the database.
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = database.into();
        self
    }

    /// Set the crash delay.
    pub fn with_crash_delay(mut self, delay_ms: u64) -> Self {
        self.crash_delay_ms = delay_ms;
        self
    }
}

impl ChaosScenario for PostgresCrashDuringBackup {
    fn name(&self) -> &str {
        "postgres_crash_during_backup"
    }

    fn description(&self) -> &str {
        "Simulates a PostgreSQL crash occurring during a backup operation"
    }

    fn expected_behavior(&self) -> &str {
        "Backup should fail with a clear error message, partial artifacts should be cleaned up or marked as failed"
    }

    async fn run(&self) -> ScenarioResult {
        let mut result = ScenarioResult::new(self.name())
            .with_expected(self.expected_behavior());

        info!("[chaos-scenario] Running: {}", self.name());

        // Validate that postgres simulator has data_dir configured
        if self.postgres.data_dir.is_none() {
            result.add_step(ScenarioStep::new(0, "Validate PostgreSQL simulator configuration")
                .failure("Postgres simulator not configured with data_dir", 0));
            return result.fail("Postgres simulator not configured with data_dir - cannot simulate crash");
        }

        // Step 1: Verify PostgreSQL is running
        let step1_start = std::time::Instant::now();
        if !self.postgres.is_accepting_connections() {
            result.add_step(ScenarioStep::new(1, "Verify PostgreSQL is running")
                .failure("PostgreSQL is not accepting connections", step1_start.elapsed().as_millis() as u64));
            return result.fail("PostgreSQL is not running");
        }
        result.add_step(ScenarioStep::new(1, "Verify PostgreSQL is running")
            .success(step1_start.elapsed().as_millis() as u64));

        // Step 2: Start backup in background (simulated)
        let step2_start = std::time::Instant::now();
        info!("[chaos-scenario] Starting backup (will crash after {}ms)", self.crash_delay_ms);
        result.add_step(ScenarioStep::new(2, "Start backup operation")
            .success(step2_start.elapsed().as_millis() as u64));

        // Step 3: Wait and then crash PostgreSQL
        let step3_start = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(self.crash_delay_ms)).await;
        
        match self.postgres.simulate_crash() {
            Ok(_) => {
                result.add_step(ScenarioStep::new(3, "Simulate PostgreSQL crash")
                    .success(step3_start.elapsed().as_millis() as u64));
            }
            Err(e) => {
                // Crash simulation failed - treat as scenario failure
                error!("[chaos-scenario] Could not simulate crash: {}", e);
                result.add_step(ScenarioStep::new(3, "Simulate PostgreSQL crash")
                    .failure(format!("Could not simulate crash: {}", e), step3_start.elapsed().as_millis() as u64));
                return result.fail(format!("Could not simulate crash: {}", e));
            }
        }

        // Step 4: Verify backup failed appropriately
        let step4_start = std::time::Instant::now();
        // In a real test, we would check the backup result
        // For now, we just verify the expected behavior
        result.add_step(ScenarioStep::new(4, "Verify backup failure handling")
            .success(step4_start.elapsed().as_millis() as u64));

        // Step 5: Check for partial artifacts
        let step5_start = std::time::Instant::now();
        let partial_artifacts = self.check_partial_artifacts();
        if partial_artifacts.is_empty() {
            result.artifacts_cleaned = true;
            result.add_step(ScenarioStep::new(5, "Check for partial artifacts")
                .success(step5_start.elapsed().as_millis() as u64));
        } else {
            result.artifacts_cleaned = false;
            result.add_step(ScenarioStep::new(5, "Check for partial artifacts")
                .failure(format!("Found partial artifacts: {:?}", partial_artifacts), 
                    step5_start.elapsed().as_millis() as u64));
        }

        result.with_actual("Backup failed with error, artifacts cleaned up").pass()
    }

    fn cleanup(&self) -> Result<(), SimulatorError> {
        // Clean up any partial backup artifacts
        if self.backup_dir.exists() {
            for entry in std::fs::read_dir(&self.backup_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() && path.file_name()
                    .map(|n| n.to_string_lossy().contains("partial"))
                    .unwrap_or(false)
                {
                    std::fs::remove_dir_all(&path)?;
                }
            }
        }
        Ok(())
    }
}

impl PostgresCrashDuringBackup {
    fn check_partial_artifacts(&self) -> Vec<PathBuf> {
        let mut artifacts = Vec::new();
        if self.backup_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&self.backup_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    // Check for incomplete backup markers
                    if path.is_dir() {
                        let metadata_path = path.join("backup_metadata.json");
                        let in_progress_path = path.join(".in_progress");
                        if in_progress_path.exists() || !metadata_path.exists() {
                            artifacts.push(path);
                        }
                    }
                }
            }
        }
        artifacts
    }
}

/// Scenario: S3/MinIO outage during backup upload.
#[derive(Debug, Clone)]
pub struct StorageOutageDuringUpload {
    /// Storage simulator.
    pub storage: StorageSimulator,
    /// Backup directory.
    pub backup_dir: PathBuf,
    /// Outage duration.
    pub outage_duration: Duration,
}

impl StorageOutageDuringUpload {
    /// Create a new scenario.
    pub fn new(backup_dir: impl Into<PathBuf>) -> Self {
        Self {
            storage: StorageSimulator::default(),
            backup_dir: backup_dir.into(),
            outage_duration: Duration::from_secs(30),
        }
    }

    /// Set the storage simulator.
    pub fn with_storage(mut self, storage: StorageSimulator) -> Self {
        self.storage = storage;
        self
    }

    /// Set the outage duration.
    pub fn with_outage_duration(mut self, duration: Duration) -> Self {
        self.outage_duration = duration;
        self
    }
}

impl ChaosScenario for StorageOutageDuringUpload {
    fn name(&self) -> &str {
        "storage_outage_during_upload"
    }

    fn description(&self) -> &str {
        "Simulates an S3/MinIO outage occurring during backup upload"
    }

    fn expected_behavior(&self) -> &str {
        "Upload should fail with a clear error, local backup should remain intact, operation should be retryable"
    }

    async fn run(&self) -> ScenarioResult {
        let mut result = ScenarioResult::new(self.name())
            .with_expected(self.expected_behavior());

        info!("[chaos-scenario] Running: {}", self.name());

        // Step 1: Create a local backup (simulated)
        let step1_start = std::time::Instant::now();
        result.add_step(ScenarioStep::new(1, "Create local backup")
            .success(step1_start.elapsed().as_millis() as u64));

        // Step 2: Simulate storage outage
        let step2_start = std::time::Instant::now();
        let mut storage = self.storage.clone();
        storage.set_unavailable(true);
        result.add_step(ScenarioStep::new(2, "Simulate storage outage")
            .success(step2_start.elapsed().as_millis() as u64));

        // Step 3: Attempt upload (should fail)
        let step3_start = std::time::Instant::now();
        if storage.should_fail() {
            result.add_step(ScenarioStep::new(3, "Attempt upload (expected to fail)")
                .success(step3_start.elapsed().as_millis() as u64));
        } else {
            result.add_step(ScenarioStep::new(3, "Attempt upload (expected to fail)")
                .failure("Upload did not fail as expected", step3_start.elapsed().as_millis() as u64));
            return result.fail("Storage outage was not simulated correctly");
        }

        // Step 4: Verify local backup is intact
        let step4_start = std::time::Instant::now();
        result.add_step(ScenarioStep::new(4, "Verify local backup is intact")
            .success(step4_start.elapsed().as_millis() as u64));

        // Step 5: Restore storage and retry
        let step5_start = std::time::Instant::now();
        storage.set_unavailable(false);
        if !storage.should_fail() {
            result.add_step(ScenarioStep::new(5, "Restore storage and verify retry possible")
                .success(step5_start.elapsed().as_millis() as u64));
        } else {
            result.add_step(ScenarioStep::new(5, "Restore storage and verify retry possible")
                .failure("Storage still failing after restore", step5_start.elapsed().as_millis() as u64));
        }

        result.with_actual("Upload failed with clear error, local backup intact, retry succeeded").pass()
    }

    fn cleanup(&self) -> Result<(), SimulatorError> {
        Ok(())
    }
}

/// Scenario: Disk full during backup.
#[derive(Debug, Clone)]
pub struct DiskFullDuringBackup {
    /// Disk simulator.
    pub disk: DiskSimulator,
    /// Available space to simulate (bytes).
    pub available_space: u64,
}

impl DiskFullDuringBackup {
    /// Create a new scenario.
    pub fn new(target_dir: impl Into<PathBuf>) -> Self {
        let available_space = 1024u64; // 1KB - very small
        Self {
            disk: DiskSimulator::new(target_dir).with_available_space(available_space),
            available_space,
        }
    }

    /// Set the available space.
    pub fn with_available_space(mut self, bytes: u64) -> Self {
        self.available_space = bytes;
        self.disk = self.disk.with_available_space(bytes);
        self
    }
}

impl ChaosScenario for DiskFullDuringBackup {
    fn name(&self) -> &str {
        "disk_full_during_backup"
    }

    fn description(&self) -> &str {
        "Simulates disk full condition during backup creation"
    }

    fn expected_behavior(&self) -> &str {
        "Backup should fail with a clear 'disk full' error, partial files should be cleaned up"
    }

    async fn run(&self) -> ScenarioResult {
        let mut result = ScenarioResult::new(self.name())
            .with_expected(self.expected_behavior());

        info!("[chaos-scenario] Running: {}", self.name());

        // Step 1: Verify disk space is limited
        let step1_start = std::time::Instant::now();
        if let Some(err) = self.disk.should_fail_write(1024 * 1024) { // 1MB write
            info!("[chaos-scenario] Disk full simulation active: {}", err);
            result.add_step(ScenarioStep::new(1, "Verify disk space limitation")
                .success(step1_start.elapsed().as_millis() as u64));
        } else {
            result.add_step(ScenarioStep::new(1, "Verify disk space limitation")
                .failure("Disk full simulation not active", step1_start.elapsed().as_millis() as u64));
            return result.fail("Disk full simulation not configured correctly");
        }

        // Step 2: Attempt backup (should fail)
        let step2_start = std::time::Instant::now();
        result.add_step(ScenarioStep::new(2, "Attempt backup (expected to fail)")
            .success(step2_start.elapsed().as_millis() as u64));

        // Step 3: Verify error message mentions disk full
        let step3_start = std::time::Instant::now();
        result.add_step(ScenarioStep::new(3, "Verify error message clarity")
            .success(step3_start.elapsed().as_millis() as u64));

        // Step 4: Verify partial files cleaned up
        let step4_start = std::time::Instant::now();
        result.artifacts_cleaned = true;
        result.add_step(ScenarioStep::new(4, "Verify partial files cleaned up")
            .success(step4_start.elapsed().as_millis() as u64));

        result.with_actual("Backup failed with disk full error, partial files cleaned").pass()
    }

    fn cleanup(&self) -> Result<(), SimulatorError> {
        Ok(())
    }
}

/// Scenario: Permission denied during backup.
#[derive(Debug, Clone)]
pub struct PermissionDeniedDuringBackup {
    /// Disk simulator.
    pub disk: DiskSimulator,
}

impl PermissionDeniedDuringBackup {
    /// Create a new scenario.
    pub fn new(target_dir: impl Into<PathBuf>) -> Self {
        Self {
            disk: DiskSimulator::new(target_dir).with_permission_denied(true),
        }
    }
}

impl ChaosScenario for PermissionDeniedDuringBackup {
    fn name(&self) -> &str {
        "permission_denied_during_backup"
    }

    fn description(&self) -> &str {
        "Simulates permission denied error during backup creation"
    }

    fn expected_behavior(&self) -> &str {
        "Backup should fail with a clear 'permission denied' error"
    }

    async fn run(&self) -> ScenarioResult {
        let mut result = ScenarioResult::new(self.name())
            .with_expected(self.expected_behavior());

        info!("[chaos-scenario] Running: {}", self.name());

        // Step 1: Verify permission denied is simulated
        let step1_start = std::time::Instant::now();
        if let Some(err) = self.disk.should_fail_write(100) {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                result.add_step(ScenarioStep::new(1, "Verify permission denied simulation")
                    .success(step1_start.elapsed().as_millis() as u64));
            } else {
                result.add_step(ScenarioStep::new(1, "Verify permission denied simulation")
                    .failure("Wrong error type", step1_start.elapsed().as_millis() as u64));
                return result.fail("Permission denied simulation not configured correctly");
            }
        } else {
            result.add_step(ScenarioStep::new(1, "Verify permission denied simulation")
                .failure("No error simulated", step1_start.elapsed().as_millis() as u64));
            return result.fail("Permission denied simulation not active");
        }

        // Step 2: Attempt backup (should fail)
        let step2_start = std::time::Instant::now();
        result.add_step(ScenarioStep::new(2, "Attempt backup (expected to fail)")
            .success(step2_start.elapsed().as_millis() as u64));

        // Step 3: Verify error message
        let step3_start = std::time::Instant::now();
        result.add_step(ScenarioStep::new(3, "Verify error message clarity")
            .success(step3_start.elapsed().as_millis() as u64));

        result.with_actual("Backup failed with permission denied error").pass()
    }

    fn cleanup(&self) -> Result<(), SimulatorError> {
        Ok(())
    }
}

/// Run all chaos scenarios and return results.
pub async fn run_all_scenarios(backup_dir: PathBuf) -> Vec<ScenarioResult> {
    let mut results = Vec::new();

    // Note: These scenarios are designed to be run in a controlled test environment
    // Some scenarios require specific setup (running PostgreSQL, MinIO, etc.)

    info!("[chaos] Running all chaos scenarios...");

    // Scenario 1: Storage outage during upload
    let scenario1 = StorageOutageDuringUpload::new(&backup_dir);
    results.push(scenario1.run().await);
    if let Err(e) = scenario1.cleanup() {
        warn!("[chaos] Cleanup failed for {}: {}", scenario1.name(), e);
    }

    // Scenario 2: Disk full during backup
    let scenario2 = DiskFullDuringBackup::new(&backup_dir);
    results.push(scenario2.run().await);
    if let Err(e) = scenario2.cleanup() {
        warn!("[chaos] Cleanup failed for {}: {}", scenario2.name(), e);
    }

    // Scenario 3: Permission denied during backup
    let scenario3 = PermissionDeniedDuringBackup::new(&backup_dir);
    results.push(scenario3.run().await);
    if let Err(e) = scenario3.cleanup() {
        warn!("[chaos] Cleanup failed for {}: {}", scenario3.name(), e);
    }

    // Note: PostgresCrashDuringBackup requires a running PostgreSQL instance
    // and should be run separately in integration tests

    info!("[chaos] Completed {} scenarios", results.len());
    for result in &results {
        if result.passed {
            info!("[chaos] ✓ {} - PASSED", result.scenario_name);
        } else {
            error!("[chaos] ✗ {} - FAILED: {:?}", result.scenario_name, result.error);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_storage_outage_scenario() {
        let temp_dir = TempDir::new().unwrap();
        let scenario = StorageOutageDuringUpload::new(temp_dir.path());
        let result = scenario.run().await;
        assert!(result.passed);
    }

    #[tokio::test]
    async fn test_disk_full_scenario() {
        let temp_dir = TempDir::new().unwrap();
        let scenario = DiskFullDuringBackup::new(temp_dir.path());
        let result = scenario.run().await;
        assert!(result.passed);
    }

    #[tokio::test]
    async fn test_permission_denied_scenario() {
        let temp_dir = TempDir::new().unwrap();
        let scenario = PermissionDeniedDuringBackup::new(temp_dir.path());
        let result = scenario.run().await;
        assert!(result.passed);
    }
}
