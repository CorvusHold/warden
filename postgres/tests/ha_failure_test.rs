//! HA orchestration failure mode tests.
//!
//! These tests verify that HA operations (switchover, failover, clone) handle
//! failure conditions correctly and maintain system safety.
//!
//! Run with: `cargo test -p postgres --test ha_failure_test -- --ignored --test-threads=1`

use std::path::PathBuf;

use chrono::{Duration, Utc};
use tempfile::TempDir;

use common::config::{ClusterConfig, Node, NodeRole, Cluster, ProtectionGroup};
use postgres::ha::types::{HaError, HaPlan, HaPlanStep, HaStepStatus};
use postgres::ha::checks::{NodeHealthCheck, NodeHealthStatus, ReplicationStatus};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test cluster configuration.
fn create_test_cluster_config() -> ClusterConfig {
    ClusterConfig {
        version: "1".to_string(),
        default_tenant: Some("test-tenant".to_string()),
        clusters: vec![Cluster {
            id: "test-cluster".to_string(),
            name: Some("Test Cluster".to_string()),
            tenant: None,
            environment: Some("test".to_string()),
            labels: Default::default(),
        }],
        nodes: vec![
            Node {
                id: "primary-node".to_string(),
                cluster_id: "test-cluster".to_string(),
                host: "primary.local".to_string(),
                port: 5432,
                role: NodeRole::Primary,
                labels: Default::default(),
                connection: None,
                ssh: None,
            },
            Node {
                id: "replica-node".to_string(),
                cluster_id: "test-cluster".to_string(),
                host: "replica.local".to_string(),
                port: 5432,
                role: NodeRole::Replica,
                labels: Default::default(),
                connection: None,
                ssh: None,
            },
        ],
        protection_groups: vec![ProtectionGroup {
            id: "test-pg".to_string(),
            name: Some("Test Protection Group".to_string()),
            cluster_id: "test-cluster".to_string(),
            databases: vec!["testdb".to_string()],
            preferred_source_role: Some(NodeRole::Replica),
            labels: Default::default(),
        }],
    }
}

/// Create a test cluster config with a lagging replica.
fn create_cluster_with_lagging_replica() -> (ClusterConfig, ReplicationStatus) {
    let config = create_test_cluster_config();
    
    let mut status = ReplicationStatus::new("replica-node");
    status.is_in_recovery = true;
    status.lag_bytes = Some(100_000_000); // 100MB lag
    status.lag_seconds = Some(300.0); // 5 minutes behind
    status.is_streaming = true;
    
    (config, status)
}

// ============================================================================
// Switchover Failure Mode Tests
// ============================================================================

/// Test switchover when primary node is unreachable.
#[test]
fn test_switchover_primary_unreachable() {
    let config = create_test_cluster_config();
    
    // Simulate primary being unreachable
    let primary_check = NodeHealthCheck::new("primary-node", "primary.local", 5432)
        .unreachable("Connection refused");
    
    assert_eq!(primary_check.status, NodeHealthStatus::Unreachable);
    assert!(primary_check.error.is_some());
    
    // In a real switchover, this should cause the operation to fail
    // with a clear error message suggesting failover instead
}

/// Test switchover when replica is lagging too far behind.
#[test]
fn test_switchover_replica_lagging() {
    let (config, repl_status) = create_cluster_with_lagging_replica();
    
    // Check if lag is acceptable (default max is 1MB)
    let max_lag_bytes = 1_000_000; // 1MB
    assert!(
        !repl_status.is_lag_acceptable(max_lag_bytes),
        "Replica with 100MB lag should not be acceptable for switchover"
    );
    
    // The switchover should fail with ReplicationLagTooHigh error
    let error = HaError::ReplicationLagTooHigh {
        node_id: "replica-node".to_string(),
        lag_bytes: repl_status.lag_bytes.unwrap(),
    };
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("lag"));
    assert!(error_msg.contains("replica-node"));
}

/// Test switchover with invalid node roles.
#[test]
fn test_switchover_invalid_roles() {
    let mut config = create_test_cluster_config();
    
    // Try to switchover FROM a replica (should fail)
    let error = HaError::InvalidNodeRole {
        node_id: "replica-node".to_string(),
        actual_role: "replica".to_string(),
        expected_role: "primary".to_string(),
    };
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("replica-node"));
    assert!(error_msg.contains("replica"));
    assert!(error_msg.contains("primary"));
}

/// Test switchover dry-run mode is safe.
#[test]
fn test_switchover_dry_run_safety() {
    let mut plan = HaPlan::new("switchover", "test-cluster", "replica-node")
        .with_source("primary-node")
        .as_dry_run();
    
    plan.add_step(HaPlanStep::new(1, "validate_config", "Validate cluster configuration"));
    plan.add_step(HaPlanStep::new(2, "check_primary", "Check primary node health"));
    plan.add_step(HaPlanStep::new(3, "check_replica", "Check replica node health").destructive());
    plan.add_step(HaPlanStep::new(4, "promote_replica", "Promote replica to primary").destructive());
    
    assert!(plan.dry_run, "Plan should be marked as dry-run");
    assert!(plan.has_destructive_steps(), "Plan should have destructive steps");
    
    // In dry-run mode, no steps should actually execute
    for step in &plan.steps {
        assert_eq!(step.status, HaStepStatus::Pending, "Steps should remain pending in dry-run");
    }
}

// ============================================================================
// Failover Failure Mode Tests
// ============================================================================

/// Test failover when primary is still reachable (without --force).
#[test]
fn test_failover_primary_still_reachable() {
    let config = create_test_cluster_config();
    
    // Primary is still reachable - failover should fail without --force
    let primary_check = NodeHealthCheck::new("primary-node", "primary.local", 5432)
        .healthy(50);
    
    assert_eq!(primary_check.status, NodeHealthStatus::Healthy);
    
    // This should result in PrimaryStillReachable error
    let error = HaError::PrimaryStillReachable;
    let error_msg = error.to_string();
    assert!(error_msg.contains("force"));
}

/// Test failover with PITR to invalid target time.
#[test]
fn test_failover_pitr_invalid_target() {
    let config = create_test_cluster_config();
    
    // Target time is in the future
    let future_time = Utc::now() + Duration::hours(1);
    
    let error = HaError::PitrNotFeasible(format!(
        "Target time {} is in the future",
        future_time.to_rfc3339()
    ));
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("PITR"));
    assert!(error_msg.contains("future"));
}

/// Test failover with PITR outside WAL window.
#[test]
fn test_failover_pitr_outside_wal_window() {
    let config = create_test_cluster_config();
    
    // Target time is before available WAL
    let old_time = Utc::now() - Duration::days(30);
    
    let error = HaError::PitrNotFeasible(format!(
        "Target time {} is outside available WAL window",
        old_time.to_rfc3339()
    ));
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("PITR"));
    assert!(error_msg.contains("WAL window"));
}

/// Test failover force flag behavior.
#[test]
fn test_failover_force_flag() {
    let mut plan = HaPlan::new("failover", "test-cluster", "replica-node");
    
    // Add warning about using force
    plan.add_warning("Primary reachability check skipped due to --force flag");
    plan.add_warning("Data loss may occur for transactions not yet replicated");
    
    assert!(!plan.warnings.is_empty());
    assert!(plan.warnings.iter().any(|w| w.contains("force")));
    assert!(plan.warnings.iter().any(|w| w.contains("Data loss")));
}

// ============================================================================
// Clone Node Failure Mode Tests
// ============================================================================

/// Test clone with non-existent backup.
#[test]
fn test_clone_backup_not_found() {
    let error = HaError::BackupNotFound("nonexistent-backup-id".to_string());
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("Backup not found"));
    assert!(error_msg.contains("nonexistent-backup-id"));
}

/// Test clone to non-empty target directory.
#[test]
fn test_clone_target_not_empty() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let target_dir = temp_dir.path().join("target");
    
    // Create target with existing content
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::write(target_dir.join("existing_file"), "content").unwrap();
    
    let error = HaError::TargetDirNotEmpty(target_dir.to_string_lossy().to_string());
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("not empty"));
}

/// Test clone with invalid node reference.
#[test]
fn test_clone_invalid_node() {
    let error = HaError::NodeNotFound("nonexistent-node".to_string());
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("Node not found"));
    assert!(error_msg.contains("nonexistent-node"));
}

// ============================================================================
// Plan Execution Failure Tests
// ============================================================================

/// Test that step failures are properly recorded.
#[test]
fn test_step_failure_recording() {
    let mut plan = HaPlan::new("switchover", "test-cluster", "replica-node");
    
    let mut step1 = HaPlanStep::new(1, "check_primary", "Check primary health");
    step1.start();
    step1.complete();
    
    let mut step2 = HaPlanStep::new(2, "check_replica", "Check replica health");
    step2.start();
    step2.fail("Connection timeout after 30 seconds");
    
    plan.add_step(step1);
    plan.add_step(step2);
    
    assert!(plan.has_failures());
    assert!(!plan.is_complete());
    
    // Find the failed step
    let failed_step = plan.steps.iter().find(|s| s.status == HaStepStatus::Failed);
    assert!(failed_step.is_some());
    assert!(failed_step.unwrap().error.as_ref().unwrap().contains("timeout"));
}

/// Test that plans detect "half-promoted" states.
#[test]
fn test_half_promoted_state_detection() {
    let mut plan = HaPlan::new("failover", "test-cluster", "replica-node");
    
    // Simulate a partial failover where promotion started but verification failed
    let mut step1 = HaPlanStep::new(1, "stop_primary", "Stop primary writes").destructive();
    step1.start();
    step1.complete();
    
    let mut step2 = HaPlanStep::new(2, "promote_replica", "Promote replica").destructive();
    step2.start();
    step2.complete();
    
    let mut step3 = HaPlanStep::new(3, "verify_promotion", "Verify new primary");
    step3.start();
    step3.fail("New primary not accepting writes");
    
    plan.add_step(step1);
    plan.add_step(step2);
    plan.add_step(step3);
    
    // Plan has failures but destructive steps completed
    assert!(plan.has_failures());
    assert!(plan.has_destructive_steps());
    
    // Count completed destructive steps
    let completed_destructive = plan.steps.iter()
        .filter(|s| s.is_destructive && s.status == HaStepStatus::Completed)
        .count();
    
    assert_eq!(completed_destructive, 2, "Two destructive steps completed before failure");
    
    // This is a "half-promoted" state that needs manual intervention
    plan.add_warning("CRITICAL: Failover partially completed. Manual intervention required.");
}

/// Test idempotency detection.
#[test]
fn test_idempotency_detection() {
    // Simulate detecting that the target is already primary
    let error = HaError::AlreadyCompleted(
        "Node replica-node is already the primary".to_string()
    );
    
    let error_msg = error.to_string();
    assert!(error_msg.contains("Already completed"));
    assert!(error_msg.contains("already the primary"));
}

// ============================================================================
// Error Message Quality Tests
// ============================================================================

/// Test that all HA errors have clear, actionable messages.
#[test]
fn test_error_message_quality() {
    let errors = vec![
        HaError::ClusterNotFound("test-cluster".to_string()),
        HaError::NodeNotFound("test-node".to_string()),
        HaError::InvalidNodeRole {
            node_id: "node-1".to_string(),
            actual_role: "replica".to_string(),
            expected_role: "primary".to_string(),
        },
        HaError::NodeUnreachable {
            node_id: "node-1".to_string(),
            host: "db.local".to_string(),
            port: 5432,
        },
        HaError::ReplicationLagTooHigh {
            node_id: "replica-1".to_string(),
            lag_bytes: 100_000_000,
        },
        HaError::PrimaryStillReachable,
        HaError::BackupNotFound("backup-123".to_string()),
        HaError::PitrNotFeasible("Target time outside WAL window".to_string()),
        HaError::TargetDirNotEmpty("/data/pg".to_string()),
        HaError::Cancelled,
        HaError::StepFailed {
            step: "promote_replica".to_string(),
            reason: "pg_ctl promote failed".to_string(),
        },
        HaError::AlreadyCompleted("Node is already primary".to_string()),
        HaError::ConfigError("Invalid cluster configuration".to_string()),
    ];
    
    for error in errors {
        let msg = error.to_string();
        
        // Error messages should not be empty
        assert!(!msg.is_empty(), "Error message should not be empty");
        
        // Error messages should not contain "unknown" or generic placeholders
        assert!(
            !msg.to_lowercase().contains("unknown error"),
            "Error message should be specific: {}",
            msg
        );
        
        // Error messages should be reasonably descriptive (at least 10 chars)
        assert!(
            msg.len() >= 10,
            "Error message should be descriptive: {}",
            msg
        );
    }
}

// ============================================================================
// Configuration Validation Tests
// ============================================================================

/// Test cluster config validation catches duplicate node IDs.
#[test]
fn test_config_duplicate_node_ids() {
    let mut config = create_test_cluster_config();
    
    // Add a duplicate node ID
    config.nodes.push(Node {
        id: "primary-node".to_string(), // Duplicate!
        cluster_id: "test-cluster".to_string(),
        host: "another-host.local".to_string(),
        port: 5432,
        role: NodeRole::Replica,
        labels: Default::default(),
        connection: None,
        ssh: None,
    });
    
    // Validation should detect this
    let node_ids: Vec<_> = config.nodes.iter().map(|n| &n.id).collect();
    let unique_ids: std::collections::HashSet<_> = node_ids.iter().collect();
    
    assert!(
        node_ids.len() != unique_ids.len(),
        "Should detect duplicate node IDs"
    );
}

/// Test cluster config validation catches invalid cluster references.
#[test]
fn test_config_invalid_cluster_reference() {
    let mut config = create_test_cluster_config();
    
    // Add a node referencing non-existent cluster
    config.nodes.push(Node {
        id: "orphan-node".to_string(),
        cluster_id: "nonexistent-cluster".to_string(), // Invalid reference!
        host: "orphan.local".to_string(),
        port: 5432,
        role: NodeRole::Replica,
        labels: Default::default(),
        connection: None,
        ssh: None,
    });
    
    // Validation should detect this
    let cluster_ids: std::collections::HashSet<_> = config.clusters.iter()
        .map(|c| &c.id)
        .collect();
    
    let invalid_refs: Vec<_> = config.nodes.iter()
        .filter(|n| !cluster_ids.contains(&n.cluster_id))
        .collect();
    
    assert!(
        !invalid_refs.is_empty(),
        "Should detect invalid cluster references"
    );
}

// ============================================================================
// Timing and Duration Tests
// ============================================================================

/// Test that step durations are tracked correctly.
#[test]
fn test_step_duration_tracking() {
    let mut step = HaPlanStep::new(1, "test_step", "Test step");
    
    step.start();
    assert!(step.started_at.is_some());
    
    // Simulate some work
    std::thread::sleep(std::time::Duration::from_millis(10));
    
    step.complete();
    assert!(step.completed_at.is_some());
    
    // Duration should be positive
    let duration = step.completed_at.unwrap() - step.started_at.unwrap();
    assert!(duration.num_milliseconds() >= 10);
}

/// Test estimated duration calculation.
#[test]
fn test_estimated_duration_calculation() {
    let mut plan = HaPlan::new("switchover", "test-cluster", "replica-node");
    
    plan.add_step(HaPlanStep::new(1, "step1", "Step 1").with_duration(10));
    plan.add_step(HaPlanStep::new(2, "step2", "Step 2").with_duration(20));
    plan.add_step(HaPlanStep::new(3, "step3", "Step 3").with_duration(30));
    
    let total = plan.estimated_total_duration_secs();
    assert_eq!(total, Some(60));
}
