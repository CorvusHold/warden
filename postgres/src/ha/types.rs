//! Common types for HA orchestration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Error type for HA operations.
#[derive(Error, Debug)]
pub enum HaError {
    #[error("Cluster not found: {0}")]
    ClusterNotFound(String),

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Invalid node role: {node_id} is {actual_role}, expected {expected_role}")]
    InvalidNodeRole {
        node_id: String,
        actual_role: String,
        expected_role: String,
    },

    #[error("Node unreachable: {node_id} at {host}:{port}")]
    NodeUnreachable {
        node_id: String,
        host: String,
        port: u16,
    },

    #[error("Replication lag too high: {lag_bytes} bytes on node {node_id}")]
    ReplicationLagTooHigh { node_id: String, lag_bytes: u64 },

    #[error("Primary still reachable (use --force to override)")]
    PrimaryStillReachable,

    #[error("Backup not found: {0}")]
    BackupNotFound(String),

    #[error("PITR not feasible: {0}")]
    PitrNotFeasible(String),

    #[error("Target directory not empty: {0}")]
    TargetDirNotEmpty(String),

    #[error("Operation cancelled by user")]
    Cancelled,

    #[error("Step failed: {step} - {reason}")]
    StepFailed { step: String, reason: String },

    #[error("Already completed: {0}")]
    AlreadyCompleted(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Postgres error: {0}")]
    Postgres(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Status of an HA plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaStepStatus {
    /// Step is pending execution.
    Pending,
    /// Step is currently executing.
    InProgress,
    /// Step completed successfully.
    Completed,
    /// Step was skipped (e.g., already done).
    Skipped,
    /// Step failed.
    Failed,
}

impl fmt::Display for HaStepStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HaStepStatus::Pending => write!(f, "pending"),
            HaStepStatus::InProgress => write!(f, "in_progress"),
            HaStepStatus::Completed => write!(f, "completed"),
            HaStepStatus::Skipped => write!(f, "skipped"),
            HaStepStatus::Failed => write!(f, "failed"),
        }
    }
}

/// A single step in an HA execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaPlanStep {
    /// Step number (1-indexed).
    pub number: usize,

    /// Short name for the step.
    pub name: String,

    /// Human-readable description of what this step does.
    pub description: String,

    /// Current status of the step.
    pub status: HaStepStatus,

    /// Estimated duration in seconds (if known).
    pub estimated_duration_secs: Option<u64>,

    /// Whether this step is destructive/irreversible.
    pub is_destructive: bool,

    /// Error message if step failed.
    pub error: Option<String>,

    /// Timestamp when step started.
    pub started_at: Option<DateTime<Utc>>,

    /// Timestamp when step completed.
    pub completed_at: Option<DateTime<Utc>>,
}

impl HaPlanStep {
    /// Create a new pending step.
    pub fn new(number: usize, name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            number,
            name: name.into(),
            description: description.into(),
            status: HaStepStatus::Pending,
            estimated_duration_secs: None,
            is_destructive: false,
            error: None,
            started_at: None,
            completed_at: None,
        }
    }

    /// Mark step as destructive.
    pub fn destructive(mut self) -> Self {
        self.is_destructive = true;
        self
    }

    /// Set estimated duration.
    pub fn with_duration(mut self, secs: u64) -> Self {
        self.estimated_duration_secs = Some(secs);
        self
    }

    /// Mark step as started.
    pub fn start(&mut self) {
        self.status = HaStepStatus::InProgress;
        self.started_at = Some(Utc::now());
    }

    /// Mark step as completed.
    pub fn complete(&mut self) {
        self.status = HaStepStatus::Completed;
        self.completed_at = Some(Utc::now());
    }

    /// Mark step as skipped.
    pub fn skip(&mut self) {
        self.status = HaStepStatus::Skipped;
        self.completed_at = Some(Utc::now());
    }

    /// Mark step as failed.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = HaStepStatus::Failed;
        self.error = Some(error.into());
        self.completed_at = Some(Utc::now());
    }
}

/// An HA execution plan containing multiple steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaPlan {
    /// Type of operation (switchover, failover, clone).
    pub operation: String,

    /// Cluster ID this plan applies to.
    pub cluster_id: String,

    /// Source node ID (if applicable).
    pub source_node_id: Option<String>,

    /// Target node ID.
    pub target_node_id: String,

    /// Steps in the plan.
    pub steps: Vec<HaPlanStep>,

    /// Whether this is a dry-run.
    pub dry_run: bool,

    /// Warnings to display to the user.
    pub warnings: Vec<String>,

    /// Plan creation timestamp.
    pub created_at: DateTime<Utc>,
}

impl HaPlan {
    /// Create a new plan.
    pub fn new(
        operation: impl Into<String>,
        cluster_id: impl Into<String>,
        target_node_id: impl Into<String>,
    ) -> Self {
        Self {
            operation: operation.into(),
            cluster_id: cluster_id.into(),
            source_node_id: None,
            target_node_id: target_node_id.into(),
            steps: Vec::new(),
            dry_run: false,
            warnings: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Set source node.
    pub fn with_source(mut self, source_node_id: impl Into<String>) -> Self {
        self.source_node_id = Some(source_node_id.into());
        self
    }

    /// Mark as dry-run.
    pub fn as_dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// Add a step to the plan.
    pub fn add_step(&mut self, step: HaPlanStep) {
        self.steps.push(step);
    }

    /// Add a warning.
    pub fn add_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }

    /// Check if plan has any destructive steps.
    pub fn has_destructive_steps(&self) -> bool {
        self.steps.iter().any(|s| s.is_destructive)
    }

    /// Get total estimated duration.
    pub fn estimated_total_duration_secs(&self) -> Option<u64> {
        let durations: Vec<u64> = self
            .steps
            .iter()
            .filter_map(|s| s.estimated_duration_secs)
            .collect();

        if durations.is_empty() {
            None
        } else {
            Some(durations.iter().sum())
        }
    }

    /// Check if all steps are completed or skipped.
    pub fn is_complete(&self) -> bool {
        self.steps
            .iter()
            .all(|s| matches!(s.status, HaStepStatus::Completed | HaStepStatus::Skipped))
    }

    /// Check if any step failed.
    pub fn has_failures(&self) -> bool {
        self.steps.iter().any(|s| s.status == HaStepStatus::Failed)
    }

    /// Get the current step (first pending or in-progress).
    pub fn current_step(&self) -> Option<&HaPlanStep> {
        self.steps
            .iter()
            .find(|s| matches!(s.status, HaStepStatus::Pending | HaStepStatus::InProgress))
    }

    /// Get mutable reference to current step.
    pub fn current_step_mut(&mut self) -> Option<&mut HaPlanStep> {
        self.steps
            .iter_mut()
            .find(|s| matches!(s.status, HaStepStatus::Pending | HaStepStatus::InProgress))
    }
}

/// Result of an HA operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaResult {
    /// Whether the operation succeeded.
    pub success: bool,

    /// Operation type.
    pub operation: String,

    /// Cluster ID.
    pub cluster_id: String,

    /// New primary node ID (after switchover/failover).
    pub new_primary_id: Option<String>,

    /// New replica node ID (after clone).
    pub new_replica_id: Option<String>,

    /// The execution plan with final step statuses.
    pub plan: HaPlan,

    /// Summary message.
    pub message: String,

    /// Completion timestamp.
    pub completed_at: DateTime<Utc>,
}

impl HaResult {
    /// Create a successful result.
    pub fn success(plan: HaPlan, message: impl Into<String>) -> Self {
        Self {
            success: true,
            operation: plan.operation.clone(),
            cluster_id: plan.cluster_id.clone(),
            new_primary_id: None,
            new_replica_id: None,
            plan,
            message: message.into(),
            completed_at: Utc::now(),
        }
    }

    /// Create a failed result.
    pub fn failure(plan: HaPlan, message: impl Into<String>) -> Self {
        Self {
            success: false,
            operation: plan.operation.clone(),
            cluster_id: plan.cluster_id.clone(),
            new_primary_id: None,
            new_replica_id: None,
            plan,
            message: message.into(),
            completed_at: Utc::now(),
        }
    }

    /// Set new primary ID.
    pub fn with_new_primary(mut self, node_id: impl Into<String>) -> Self {
        self.new_primary_id = Some(node_id.into());
        self
    }

    /// Set new replica ID.
    pub fn with_new_replica(mut self, node_id: impl Into<String>) -> Self {
        self.new_replica_id = Some(node_id.into());
        self
    }
}

/// Format a plan for display.
pub fn format_plan(plan: &HaPlan, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(plan).unwrap_or_default(),
        _ => format_plan_table(plan),
    }
}

fn format_plan_table(plan: &HaPlan) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&format!(
        "\n{} Plan: {} -> {}\n",
        plan.operation.to_uppercase(),
        plan.source_node_id.as_deref().unwrap_or("N/A"),
        plan.target_node_id
    ));
    output.push_str(&format!("Cluster: {}\n", plan.cluster_id));

    if plan.dry_run {
        output.push_str("Mode: DRY-RUN (no changes will be made)\n");
    }
    output.push('\n');

    // Warnings
    if !plan.warnings.is_empty() {
        output.push_str("⚠️  WARNINGS:\n");
        for warning in &plan.warnings {
            output.push_str(&format!("   - {}\n", warning));
        }
        output.push('\n');
    }

    // Steps
    output.push_str("Execution Plan:\n");
    output.push_str(&format!(
        "{:<4} {:<30} {:<12} {:<50}\n",
        "#", "STEP", "STATUS", "DESCRIPTION"
    ));
    output.push_str(&"-".repeat(100));
    output.push('\n');

    for step in &plan.steps {
        let status_icon = match step.status {
            HaStepStatus::Pending => "○",
            HaStepStatus::InProgress => "◐",
            HaStepStatus::Completed => "✓",
            HaStepStatus::Skipped => "⊘",
            HaStepStatus::Failed => "✗",
        };

        let destructive_marker = if step.is_destructive { " ⚠" } else { "" };

        output.push_str(&format!(
            "{:<4} {:<30} {:<12} {:<50}\n",
            step.number,
            format!("{}{}", step.name, destructive_marker),
            format!("{} {}", status_icon, step.status),
            step.description,
        ));

        if let Some(error) = &step.error {
            output.push_str(&format!("     └─ Error: {}\n", error));
        }
    }

    // Summary
    if let Some(duration) = plan.estimated_total_duration_secs() {
        output.push_str(&format!("\nEstimated duration: {} seconds\n", duration));
    }

    if plan.has_destructive_steps() {
        output.push_str("\n⚠️  This plan contains destructive steps that cannot be undone.\n");
    }

    output
}

/// Format a result for display.
pub fn format_result(result: &HaResult, format: &str) -> String {
    match format {
        "json" => serde_json::to_string_pretty(result).unwrap_or_default(),
        _ => format_result_table(result),
    }
}

fn format_result_table(result: &HaResult) -> String {
    let mut output = String::new();

    let status_icon = if result.success { "✓" } else { "✗" };
    output.push_str(&format!(
        "\n{} {} {}\n",
        status_icon,
        result.operation.to_uppercase(),
        if result.success {
            "COMPLETED"
        } else {
            "FAILED"
        }
    ));
    output.push_str(&format!("Cluster: {}\n", result.cluster_id));

    if let Some(ref primary) = result.new_primary_id {
        output.push_str(&format!("New Primary: {}\n", primary));
    }
    if let Some(ref replica) = result.new_replica_id {
        output.push_str(&format!("New Replica: {}\n", replica));
    }

    output.push_str(&format!("\nMessage: {}\n", result.message));
    output.push_str(&format!("Completed at: {}\n", result.completed_at));

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_step_lifecycle() {
        let mut step = HaPlanStep::new(1, "test", "Test step");
        assert_eq!(step.status, HaStepStatus::Pending);

        step.start();
        assert_eq!(step.status, HaStepStatus::InProgress);
        assert!(step.started_at.is_some());

        step.complete();
        assert_eq!(step.status, HaStepStatus::Completed);
        assert!(step.completed_at.is_some());
    }

    #[test]
    fn test_plan_completion() {
        let mut plan = HaPlan::new("switchover", "test-cluster", "node-2");
        plan.add_step(HaPlanStep::new(1, "step1", "First step"));
        plan.add_step(HaPlanStep::new(2, "step2", "Second step"));

        assert!(!plan.is_complete());

        plan.steps[0].complete();
        assert!(!plan.is_complete());

        plan.steps[1].complete();
        assert!(plan.is_complete());
    }

    #[test]
    fn test_plan_has_failures() {
        let mut plan = HaPlan::new("failover", "test-cluster", "node-2");
        plan.add_step(HaPlanStep::new(1, "step1", "First step"));

        assert!(!plan.has_failures());

        plan.steps[0].fail("Something went wrong");
        assert!(plan.has_failures());
    }
}
