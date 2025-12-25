//! Unit tests for Full Restore functionality
//!
//! These tests verify the restore planning logic without requiring
//! an actual PostgreSQL instance or S3 storage.

use postgres::common::PostgresConfig;
use postgres::restore::full_restore::{
    BackupInfo, BackupSource, FullRestoreManager, PreflightError, PreflightResult,
    PreflightWarning, RestoreAction, RestoreMode, RestorePlan, RestoreStep, TargetState,
    ToolsAvailability,
};
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper to create a test PostgresConfig
fn test_config() -> PostgresConfig {
    PostgresConfig {
        host: "localhost".to_string(),
        port: 5432,
        database: "testdb".to_string(),
        user: "postgres".to_string(),
        password: Some("password".to_string()),
        ssl_mode: None,
        maintenance_db: Some("postgres".to_string()),
        ssh_host: None,
        ssh_user: None,
        ssh_port: None,
        ssh_password: None,
        ssh_key_path: None,
        ssh_local_port: None,
        ssh_remote_port: None,
    }
}

/// Helper to create a test backup directory with a dump file
fn setup_test_backup(backup_id: &str) -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let backup_path = temp_dir.path().join(backup_id);
    std::fs::create_dir_all(&backup_path).expect("Failed to create backup dir");

    // Create a dummy dump file
    let dump_file = backup_path.join("testdb.dump");
    std::fs::write(&dump_file, b"PGDUMP test content").expect("Failed to write dump file");

    // Create metadata file
    let metadata = serde_json::json!({
        "backup_id": backup_id,
        "backup_type": "snapshot",
        "database": "testdb",
        "size_bytes": 1024,
        "start_time": "2025-01-01T00:00:00Z",
        "server_version": "15.0"
    });
    let metadata_path = backup_path.join("backup_metadata.json");
    std::fs::write(&metadata_path, metadata.to_string()).expect("Failed to write metadata");

    (temp_dir, backup_path)
}

// ============================================================================
// PreflightResult Tests
// ============================================================================

#[test]
fn test_preflight_result_new_is_passed() {
    let result = PreflightResult::new();
    assert!(result.passed);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
    assert!(result.backup_info.is_none());
}

#[test]
fn test_preflight_result_add_error_fails() {
    let mut result = PreflightResult::new();
    result.add_error(PreflightError::new("TEST_ERROR", "Test error message"));

    assert!(!result.passed);
    assert_eq!(result.errors.len(), 1);
    assert_eq!(result.errors[0].code, "TEST_ERROR");
    assert_eq!(result.errors[0].message, "Test error message");
}

#[test]
fn test_preflight_result_add_warning_still_passes() {
    let mut result = PreflightResult::new();
    result.add_warning(PreflightWarning::new(
        "TEST_WARNING",
        "Test warning message",
    ));

    assert!(result.passed); // Warnings don't fail preflight
    assert!(result.errors.is_empty());
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.warnings[0].code, "TEST_WARNING");
}

#[test]
fn test_preflight_error_with_details() {
    let error = PreflightError::new("CODE", "Message").with_details("Additional details");

    assert_eq!(error.code, "CODE");
    assert_eq!(error.message, "Message");
    assert_eq!(error.details, Some("Additional details".to_string()));
}

#[test]
fn test_preflight_warning_with_recommendation() {
    let warning = PreflightWarning::new("CODE", "Message").with_recommendation("Do this instead");

    assert_eq!(warning.code, "CODE");
    assert_eq!(warning.message, "Message");
    assert_eq!(warning.recommendation, Some("Do this instead".to_string()));
}

// ============================================================================
// RestoreMode Tests
// ============================================================================

#[test]
fn test_restore_mode_default_is_replace() {
    let mode = RestoreMode::default();
    assert_eq!(mode, RestoreMode::Replace);
}

#[test]
fn test_restore_mode_new_database() {
    let mode = RestoreMode::NewDatabase {
        target_name: "new_db".to_string(),
    };

    match mode {
        RestoreMode::NewDatabase { target_name } => {
            assert_eq!(target_name, "new_db");
        }
        _ => panic!("Expected NewDatabase mode"),
    }
}

#[test]
fn test_restore_mode_equality() {
    assert_eq!(RestoreMode::Replace, RestoreMode::Replace);
    assert_ne!(
        RestoreMode::Replace,
        RestoreMode::NewDatabase {
            target_name: "db".to_string()
        }
    );
}

// ============================================================================
// TargetState Tests
// ============================================================================

#[test]
fn test_target_state_variants() {
    assert_eq!(TargetState::Empty, TargetState::Empty);
    assert_eq!(TargetState::NotExists, TargetState::NotExists);
    assert_eq!(TargetState::NonEmpty, TargetState::NonEmpty);

    let cluster_state = TargetState::PostgresCluster {
        version: Some("15".to_string()),
    };
    match cluster_state {
        TargetState::PostgresCluster { version } => {
            assert_eq!(version, Some("15".to_string()));
        }
        _ => panic!("Expected PostgresCluster state"),
    }
}

// ============================================================================
// BackupInfo Tests
// ============================================================================

#[test]
fn test_backup_info_local_source() {
    let info = BackupInfo {
        id: "test-backup".to_string(),
        backup_type: "snapshot".to_string(),
        database: Some("mydb".to_string()),
        size_bytes: 1024 * 1024,
        created_at: chrono::Utc::now(),
        server_version: Some("15.0".to_string()),
        source: BackupSource::Local {
            path: PathBuf::from("/backups/test"),
        },
    };

    assert_eq!(info.id, "test-backup");
    assert_eq!(info.size_bytes, 1024 * 1024);
    match info.source {
        BackupSource::Local { path } => {
            assert_eq!(path, PathBuf::from("/backups/test"));
        }
        _ => panic!("Expected Local source"),
    }
}

#[test]
fn test_backup_info_remote_source() {
    let info = BackupInfo {
        id: "test-backup".to_string(),
        backup_type: "full".to_string(),
        database: None,
        size_bytes: 0,
        created_at: chrono::Utc::now(),
        server_version: None,
        source: BackupSource::Remote {
            bucket: "my-bucket".to_string(),
            key: "backups/test".to_string(),
        },
    };

    match info.source {
        BackupSource::Remote { bucket, key } => {
            assert_eq!(bucket, "my-bucket");
            assert_eq!(key, "backups/test");
        }
        _ => panic!("Expected Remote source"),
    }
}

// ============================================================================
// RestorePlan Tests
// ============================================================================

#[test]
fn test_restore_plan_structure() {
    let plan = RestorePlan {
        id: "plan-123".to_string(),
        backup_id: "backup-456".to_string(),
        target_config: postgres::restore::full_restore::TargetConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "testdb".to_string(),
            user: "postgres".to_string(),
            ssl_mode: None,
        },
        mode: RestoreMode::Replace,
        steps: vec![
            RestoreStep {
                order: 1,
                action: RestoreAction::ValidateBackup {
                    backup_path: PathBuf::from("/backups/test"),
                },
                description: "Validate backup".to_string(),
                reversible: false,
            },
            RestoreStep {
                order: 2,
                action: RestoreAction::CreateDatabase {
                    database: "testdb".to_string(),
                    owner: Some("postgres".to_string()),
                },
                description: "Create database".to_string(),
                reversible: true,
            },
        ],
        estimated_duration_secs: Some(60),
        requires_confirmation: true,
        confirmation_reason: Some("Destructive operation".to_string()),
    };

    assert_eq!(plan.id, "plan-123");
    assert_eq!(plan.backup_id, "backup-456");
    assert_eq!(plan.steps.len(), 2);
    assert!(plan.requires_confirmation);
}

#[test]
fn test_restore_step_reversibility() {
    let reversible_step = RestoreStep {
        order: 1,
        action: RestoreAction::CreateDatabase {
            database: "test".to_string(),
            owner: None,
        },
        description: "Create database".to_string(),
        reversible: true,
    };

    let irreversible_step = RestoreStep {
        order: 2,
        action: RestoreAction::DropDatabase {
            database: "test".to_string(),
        },
        description: "Drop database".to_string(),
        reversible: false,
    };

    assert!(reversible_step.reversible);
    assert!(!irreversible_step.reversible);
}

// ============================================================================
// RestoreAction Tests
// ============================================================================

#[test]
fn test_restore_action_variants() {
    // Test each action variant can be created
    let actions = vec![
        RestoreAction::DownloadBackup {
            backup_id: "id".to_string(),
            target_path: PathBuf::from("/tmp"),
        },
        RestoreAction::ValidateBackup {
            backup_path: PathBuf::from("/backups"),
        },
        RestoreAction::TerminateConnections {
            database: "db".to_string(),
        },
        RestoreAction::DropDatabase {
            database: "db".to_string(),
        },
        RestoreAction::CreateDatabase {
            database: "db".to_string(),
            owner: Some("user".to_string()),
        },
        RestoreAction::RestoreContent {
            dump_path: PathBuf::from("/dump.sql"),
            database: "db".to_string(),
        },
        RestoreAction::HealthCheck {
            database: "db".to_string(),
            timeout_secs: 30,
        },
        RestoreAction::Cleanup {
            paths: vec![PathBuf::from("/tmp/cleanup")],
        },
    ];

    assert_eq!(actions.len(), 8);
}

// ============================================================================
// ToolsAvailability Tests
// ============================================================================

#[test]
fn test_tools_availability_default() {
    let tools = ToolsAvailability::default();

    assert!(tools.pg_restore.is_none());
    assert!(tools.psql.is_none());
    assert!(tools.pg_isready.is_none());
    assert!(tools.pg_ctl.is_none());
    assert!(tools.createdb.is_none());
    assert!(tools.dropdb.is_none());
}

// ============================================================================
// FullRestoreManager Tests
// ============================================================================

#[test]
fn test_full_restore_manager_creation() {
    let config = test_config();
    let backup_dir = PathBuf::from("/tmp/backups");

    let manager = FullRestoreManager::new(config.clone(), backup_dir.clone());

    // Manager should be created successfully
    // We can't easily inspect internal state, but we can verify it doesn't panic
    let _ = manager.with_mode(RestoreMode::Replace).with_force(true);
}

#[test]
fn test_full_restore_manager_with_mode() {
    let config = test_config();
    let backup_dir = PathBuf::from("/tmp/backups");

    let manager = FullRestoreManager::new(config, backup_dir).with_mode(RestoreMode::NewDatabase {
        target_name: "new_db".to_string(),
    });

    // Verify chaining works
    let _ = manager.with_force(false);
}

#[tokio::test]
async fn test_preflight_backup_not_found() {
    let config = test_config();
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf());

    let result = manager.preflight("nonexistent-backup", None).await;

    assert!(result.is_ok());
    let preflight = result.unwrap();

    // Should fail because backup doesn't exist
    assert!(!preflight.passed);
    assert!(preflight
        .errors
        .iter()
        .any(|e| e.code == "BACKUP_NOT_FOUND"));
}

#[tokio::test]
async fn test_preflight_backup_found() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup-123");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf());

    let result = manager.preflight("test-backup-123", None).await;

    assert!(result.is_ok());
    let preflight = result.unwrap();

    // Should have backup info
    assert!(preflight.backup_info.is_some());
    let backup_info = preflight.backup_info.unwrap();
    assert_eq!(backup_info.id, "test-backup-123");
}

#[tokio::test]
async fn test_preflight_target_not_exists() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf());

    let nonexistent_target = temp_dir.path().join("nonexistent");
    let result = manager
        .preflight("test-backup", Some(&nonexistent_target))
        .await;

    assert!(result.is_ok());
    let preflight = result.unwrap();
    assert_eq!(preflight.target_state, TargetState::NotExists);
}

#[tokio::test]
async fn test_preflight_target_empty() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    // Create empty target directory
    let target_dir = temp_dir.path().join("empty_target");
    std::fs::create_dir_all(&target_dir).expect("Failed to create target dir");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf());

    let result = manager.preflight("test-backup", Some(&target_dir)).await;

    assert!(result.is_ok());
    let preflight = result.unwrap();
    assert_eq!(preflight.target_state, TargetState::Empty);
}

#[tokio::test]
async fn test_preflight_target_non_empty_without_force() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    // Create non-empty target directory
    let target_dir = temp_dir.path().join("non_empty_target");
    std::fs::create_dir_all(&target_dir).expect("Failed to create target dir");
    std::fs::write(target_dir.join("some_file.txt"), "content").expect("Failed to write file");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf());

    let result = manager.preflight("test-backup", Some(&target_dir)).await;

    assert!(result.is_ok());
    let preflight = result.unwrap();
    assert_eq!(preflight.target_state, TargetState::NonEmpty);
    // Should have an error because force is not set
    assert!(preflight
        .errors
        .iter()
        .any(|e| e.code == "TARGET_NOT_EMPTY"));
}

#[tokio::test]
async fn test_preflight_target_non_empty_with_force() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    // Create non-empty target directory
    let target_dir = temp_dir.path().join("non_empty_target");
    std::fs::create_dir_all(&target_dir).expect("Failed to create target dir");
    std::fs::write(target_dir.join("some_file.txt"), "content").expect("Failed to write file");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf()).with_force(true);

    let result = manager.preflight("test-backup", Some(&target_dir)).await;

    assert!(result.is_ok());
    let preflight = result.unwrap();
    // With force, should not have TARGET_NOT_EMPTY error
    assert!(!preflight
        .errors
        .iter()
        .any(|e| e.code == "TARGET_NOT_EMPTY"));
}

#[tokio::test]
async fn test_preflight_postgres_cluster_without_force() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    // Create target directory that looks like a PostgreSQL cluster
    let target_dir = temp_dir.path().join("pg_cluster");
    std::fs::create_dir_all(&target_dir).expect("Failed to create target dir");
    std::fs::write(target_dir.join("PG_VERSION"), "15").expect("Failed to write PG_VERSION");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf());

    let result = manager.preflight("test-backup", Some(&target_dir)).await;

    assert!(result.is_ok());
    let preflight = result.unwrap();

    match &preflight.target_state {
        TargetState::PostgresCluster { version } => {
            assert_eq!(version.as_deref(), Some("15"));
        }
        _ => panic!("Expected PostgresCluster state"),
    }

    // Should have an error because force is not set
    assert!(preflight
        .errors
        .iter()
        .any(|e| e.code == "TARGET_HAS_CLUSTER"));
}

#[test]
fn test_create_plan_requires_passed_preflight() {
    let config = test_config();
    let backup_dir = PathBuf::from("/tmp/backups");

    let manager = FullRestoreManager::new(config, backup_dir);

    // Create a failed preflight result
    let mut preflight = PreflightResult::new();
    preflight.add_error(PreflightError::new("TEST", "Test error"));

    let result = manager.create_plan("backup-id", &preflight);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("preflight validation failed"));
}

#[test]
fn test_create_plan_requires_backup_info() {
    let config = test_config();
    let backup_dir = PathBuf::from("/tmp/backups");

    let manager = FullRestoreManager::new(config, backup_dir);

    // Create a passed preflight result but without backup info
    let preflight = PreflightResult::new();

    let result = manager.create_plan("backup-id", &preflight);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No backup information"));
}

#[tokio::test]
async fn test_create_plan_replace_mode() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf())
        .with_mode(RestoreMode::Replace);

    let preflight = manager.preflight("test-backup", None).await.unwrap();

    // Skip if preflight failed (e.g., missing tools)
    if !preflight.passed {
        return;
    }

    let plan = manager.create_plan("test-backup", &preflight).unwrap();

    // Replace mode should include terminate connections and drop database steps
    assert!(plan
        .steps
        .iter()
        .any(|s| matches!(&s.action, RestoreAction::TerminateConnections { .. })));
    assert!(plan
        .steps
        .iter()
        .any(|s| matches!(&s.action, RestoreAction::DropDatabase { .. })));
    assert!(plan.requires_confirmation);
}

#[tokio::test]
async fn test_create_plan_new_database_mode() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf()).with_mode(
        RestoreMode::NewDatabase {
            target_name: "new_testdb".to_string(),
        },
    );

    let preflight = manager.preflight("test-backup", None).await.unwrap();

    // Skip if preflight failed (e.g., missing tools)
    if !preflight.passed {
        return;
    }

    let plan = manager.create_plan("test-backup", &preflight).unwrap();

    // NewDatabase mode should NOT include terminate connections or drop database
    assert!(!plan
        .steps
        .iter()
        .any(|s| matches!(&s.action, RestoreAction::TerminateConnections { .. })));
    assert!(!plan
        .steps
        .iter()
        .any(|s| matches!(&s.action, RestoreAction::DropDatabase { .. })));

    // Should create the new database
    assert!(plan.steps.iter().any(|s| matches!(
        &s.action,
        RestoreAction::CreateDatabase { database, .. } if database == "new_testdb"
    )));
}

#[tokio::test]
async fn test_create_plan_with_force_no_confirmation() {
    let config = test_config();
    let (temp_dir, _backup_path) = setup_test_backup("test-backup");

    let manager = FullRestoreManager::new(config, temp_dir.path().to_path_buf())
        .with_mode(RestoreMode::Replace)
        .with_force(true);

    let preflight = manager.preflight("test-backup", None).await.unwrap();

    // Skip if preflight failed
    if !preflight.passed {
        return;
    }

    let plan = manager.create_plan("test-backup", &preflight).unwrap();

    // With force, should not require confirmation
    assert!(!plan.requires_confirmation);
}

// ============================================================================
// Serialization Tests
// ============================================================================

#[test]
fn test_preflight_result_serialization() {
    let mut result = PreflightResult::new();
    result.add_warning(PreflightWarning::new("WARN", "Warning message"));

    let json = serde_json::to_string(&result);
    assert!(json.is_ok());

    let deserialized: PreflightResult = serde_json::from_str(&json.unwrap()).unwrap();
    assert!(deserialized.passed);
    assert_eq!(deserialized.warnings.len(), 1);
}

#[test]
fn test_restore_mode_serialization() {
    let mode = RestoreMode::NewDatabase {
        target_name: "test".to_string(),
    };

    let json = serde_json::to_string(&mode).unwrap();
    let deserialized: RestoreMode = serde_json::from_str(&json).unwrap();

    assert_eq!(mode, deserialized);
}

#[test]
fn test_backup_source_serialization() {
    let local = BackupSource::Local {
        path: PathBuf::from("/backups/test"),
    };
    let remote = BackupSource::Remote {
        bucket: "bucket".to_string(),
        key: "key".to_string(),
    };

    let local_json = serde_json::to_string(&local).unwrap();
    let remote_json = serde_json::to_string(&remote).unwrap();

    let local_de: BackupSource = serde_json::from_str(&local_json).unwrap();
    let remote_de: BackupSource = serde_json::from_str(&remote_json).unwrap();

    match local_de {
        BackupSource::Local { path } => assert_eq!(path, PathBuf::from("/backups/test")),
        _ => panic!("Expected Local"),
    }

    match remote_de {
        BackupSource::Remote { bucket, key } => {
            assert_eq!(bucket, "bucket");
            assert_eq!(key, "key");
        }
        _ => panic!("Expected Remote"),
    }
}
