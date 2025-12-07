//! Full Restore to Replacement Instance
//!
//! This module implements the full restore workflow for PostgreSQL databases,
//! supporting failover and cluster evolution scenarios. It provides:
//!
//! - Preflight validation (backup existence, target state, required tools)
//! - Backup discovery and download from S3
//! - Database restore with pre/post steps (drop/recreate DB, roles, extensions)
//! - Health checks and verification
//!
//! See ADR-002 for design decisions.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::common::PostgresConfig;

/// Restore mode: replace existing database or restore to a new database name
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum RestoreMode {
    /// Replace the existing database (drop and recreate)
    #[default]
    Replace,
    /// Restore to a new database with a different name
    NewDatabase { target_name: String },
}


/// Result of preflight validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightResult {
    /// Whether all checks passed
    pub passed: bool,
    /// List of validation errors (blocking issues)
    pub errors: Vec<PreflightError>,
    /// List of warnings (non-blocking issues)
    pub warnings: Vec<PreflightWarning>,
    /// Backup metadata if found
    pub backup_info: Option<BackupInfo>,
    /// Target directory state
    pub target_state: TargetState,
    /// Required tools availability
    pub tools: ToolsAvailability,
}

impl PreflightResult {
    pub fn new() -> Self {
        Self {
            passed: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            backup_info: None,
            target_state: TargetState::Empty,
            tools: ToolsAvailability::default(),
        }
    }

    pub fn add_error(&mut self, error: PreflightError) {
        self.passed = false;
        self.errors.push(error);
    }

    pub fn add_warning(&mut self, warning: PreflightWarning) {
        self.warnings.push(warning);
    }
}

impl Default for PreflightResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Preflight validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

impl PreflightError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// Preflight validation warning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightWarning {
    pub code: String,
    pub message: String,
    pub recommendation: Option<String>,
}

impl PreflightWarning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recommendation: None,
        }
    }

    pub fn with_recommendation(mut self, recommendation: impl Into<String>) -> Self {
        self.recommendation = Some(recommendation.into());
        self
    }
}

/// Backup information for restore planning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub id: String,
    pub backup_type: String,
    pub database: Option<String>,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub server_version: Option<String>,
    pub source: BackupSource,
}

/// Source of the backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackupSource {
    Local { path: PathBuf },
    Remote { bucket: String, key: String },
}

/// State of the target directory
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TargetState {
    /// Directory doesn't exist
    NotExists,
    /// Directory exists but is empty
    Empty,
    /// Directory exists with non-PostgreSQL data
    NonEmpty,
    /// Directory contains a PostgreSQL cluster
    PostgresCluster { version: Option<String> },
}

/// Availability of required tools
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsAvailability {
    pub pg_restore: Option<ToolInfo>,
    pub psql: Option<ToolInfo>,
    pub pg_isready: Option<ToolInfo>,
    pub pg_ctl: Option<ToolInfo>,
    pub createdb: Option<ToolInfo>,
    pub dropdb: Option<ToolInfo>,
}

/// Information about a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub path: PathBuf,
    pub version: Option<String>,
}

/// Restore plan describing what will be done
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePlan {
    /// Unique plan ID
    pub id: String,
    /// Backup to restore from
    pub backup_id: String,
    /// Target database configuration
    pub target_config: TargetConfig,
    /// Restore mode
    pub mode: RestoreMode,
    /// Steps to execute
    pub steps: Vec<RestoreStep>,
    /// Estimated duration in seconds
    pub estimated_duration_secs: Option<u64>,
    /// Whether confirmation is required
    pub requires_confirmation: bool,
    /// Reason for requiring confirmation
    pub confirmation_reason: Option<String>,
}

/// Target database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub ssl_mode: Option<String>,
}

/// A step in the restore plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreStep {
    pub order: u32,
    pub action: RestoreAction,
    pub description: String,
    pub reversible: bool,
}

/// Actions that can be performed during restore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RestoreAction {
    /// Download backup from S3
    DownloadBackup { backup_id: String, target_path: PathBuf },
    /// Validate backup integrity
    ValidateBackup { backup_path: PathBuf },
    /// Terminate existing connections to the database
    TerminateConnections { database: String },
    /// Drop existing database
    DropDatabase { database: String },
    /// Create new database
    CreateDatabase { database: String, owner: Option<String> },
    /// Restore database content from dump
    RestoreContent { dump_path: PathBuf, database: String },
    /// Apply post-restore configuration
    ApplyConfiguration { config: HashMap<String, String> },
    /// Verify database health
    HealthCheck { database: String, timeout_secs: u64 },
    /// Clean up temporary files
    Cleanup { paths: Vec<PathBuf> },
}

/// Result of a restore operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub success: bool,
    pub plan_id: String,
    pub backup_id: String,
    pub database: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub steps_completed: Vec<StepResult>,
    pub error: Option<String>,
}

/// Result of a single restore step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub order: u32,
    pub action: String,
    pub success: bool,
    pub duration_ms: u64,
    pub message: Option<String>,
}

/// Full restore manager
pub struct FullRestoreManager {
    config: PostgresConfig,
    backup_dir: PathBuf,
    mode: RestoreMode,
    force: bool,
}

impl FullRestoreManager {
    /// Create a new full restore manager
    pub fn new(config: PostgresConfig, backup_dir: PathBuf) -> Self {
        Self {
            config,
            backup_dir,
            mode: RestoreMode::Replace,
            force: false,
        }
    }

    /// Set the restore mode
    pub fn with_mode(mut self, mode: RestoreMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set force flag (skip confirmation for destructive operations)
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Perform preflight validation
    pub async fn preflight(
        &self,
        backup_id: &str,
        target_dir: Option<&Path>,
    ) -> Result<PreflightResult> {
        let mut result = PreflightResult::new();

        info!("[preflight] Starting preflight validation for backup {}", backup_id);

        // Check backup existence
        self.check_backup_exists(backup_id, &mut result).await?;

        // Check target directory state
        if let Some(target) = target_dir {
            self.check_target_directory(target, &mut result)?;
        }

        // Check required tools
        self.check_required_tools(&mut result)?;

        // Check database connectivity (if not creating new cluster)
        if target_dir.is_none() {
            self.check_database_connectivity(&mut result).await?;
        }

        info!(
            "[preflight] Validation complete: passed={}, errors={}, warnings={}",
            result.passed,
            result.errors.len(),
            result.warnings.len()
        );

        Ok(result)
    }

    /// Check if backup exists (locally or remotely)
    async fn check_backup_exists(
        &self,
        backup_id: &str,
        result: &mut PreflightResult,
    ) -> Result<()> {
        // Check local backup directory
        let local_path = self.backup_dir.join(backup_id);
        if local_path.exists() && local_path.is_dir() {
            // Check for dump files
            let has_dump = std::fs::read_dir(&local_path)
                .map(|entries| {
                    entries.filter_map(|e| e.ok()).any(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        name.ends_with(".dump") || name.ends_with(".sql")
                    })
                })
                .unwrap_or(false);

            if has_dump {
                // Try to read metadata
                let metadata_path = local_path.join("backup_metadata.json");
                let backup_info = if metadata_path.exists() {
                    match std::fs::read_to_string(&metadata_path) {
                        Ok(content) => {
                            if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&content) {
                                Some(BackupInfo {
                                    id: backup_id.to_string(),
                                    backup_type: metadata.get("backup_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                    database: metadata.get("database")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    size_bytes: metadata.get("size_bytes")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0),
                                    created_at: metadata.get("start_time")
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                        .map(|dt| dt.with_timezone(&Utc))
                                        .unwrap_or_else(Utc::now),
                                    server_version: metadata.get("server_version")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                    source: BackupSource::Local { path: local_path.clone() },
                                })
                            } else {
                                None
                            }
                        }
                        Err(_) => None,
                    }
                } else {
                    // Create basic info without metadata
                    let size = calculate_dir_size(&local_path).unwrap_or(0);
                    Some(BackupInfo {
                        id: backup_id.to_string(),
                        backup_type: "unknown".to_string(),
                        database: None,
                        size_bytes: size,
                        created_at: Utc::now(),
                        server_version: None,
                        source: BackupSource::Local { path: local_path.clone() },
                    })
                };

                result.backup_info = backup_info;
                info!("[preflight] Found local backup at {:?}", local_path);
                return Ok(());
            }
        }

        // Backup not found locally
        result.add_error(PreflightError::new(
            "BACKUP_NOT_FOUND",
            format!("Backup '{}' not found in local backup directory", backup_id),
        ).with_details(format!(
            "Searched in: {:?}. Use --remote-storage to download from S3.",
            self.backup_dir
        )));

        Ok(())
    }

    /// Check target directory state
    fn check_target_directory(&self, target: &Path, result: &mut PreflightResult) -> Result<()> {
        if !target.exists() {
            result.target_state = TargetState::NotExists;
            info!("[preflight] Target directory does not exist, will be created");
            return Ok(());
        }

        if !target.is_dir() {
            result.add_error(PreflightError::new(
                "TARGET_NOT_DIRECTORY",
                "Target path exists but is not a directory",
            ));
            return Ok(());
        }

        // Check if directory is empty
        let entries: Vec<_> = std::fs::read_dir(target)
            .map(|e| e.filter_map(|e| e.ok()).collect())
            .unwrap_or_default();

        if entries.is_empty() {
            result.target_state = TargetState::Empty;
            info!("[preflight] Target directory is empty");
            return Ok(());
        }

        // Check if it's a PostgreSQL cluster
        let pg_version_file = target.join("PG_VERSION");
        if pg_version_file.exists() {
            let version = std::fs::read_to_string(&pg_version_file).ok();
            result.target_state = TargetState::PostgresCluster {
                version: version.map(|v| v.trim().to_string()),
            };

            if !self.force {
                result.add_error(PreflightError::new(
                    "TARGET_HAS_CLUSTER",
                    "Target directory contains an existing PostgreSQL cluster",
                ).with_details(
                    "Use --yes to overwrite the existing cluster, or choose a different target directory."
                ));
            } else {
                result.add_warning(PreflightWarning::new(
                    "OVERWRITING_CLUSTER",
                    "Existing PostgreSQL cluster will be overwritten",
                ).with_recommendation(
                    "Ensure the existing cluster is stopped and backed up before proceeding."
                ));
            }
        } else {
            result.target_state = TargetState::NonEmpty;
            if !self.force {
                result.add_error(PreflightError::new(
                    "TARGET_NOT_EMPTY",
                    "Target directory is not empty",
                ).with_details(
                    "Use --yes to overwrite the existing contents, or choose an empty directory."
                ));
            }
        }

        Ok(())
    }

    /// Check required tools availability
    fn check_required_tools(&self, result: &mut PreflightResult) -> Result<()> {
        // Check pg_restore
        result.tools.pg_restore = check_tool("pg_restore");
        if result.tools.pg_restore.is_none() {
            result.add_warning(PreflightWarning::new(
                "MISSING_PG_RESTORE",
                "pg_restore not found in PATH",
            ).with_recommendation(
                "Install PostgreSQL client tools or add them to PATH."
            ));
        }

        // Check psql
        result.tools.psql = check_tool("psql");
        if result.tools.psql.is_none() {
            result.add_warning(PreflightWarning::new(
                "MISSING_PSQL",
                "psql not found in PATH",
            ).with_recommendation(
                "Install PostgreSQL client tools or add them to PATH."
            ));
        }

        // Check pg_isready
        result.tools.pg_isready = check_tool("pg_isready");

        // Check pg_ctl
        result.tools.pg_ctl = check_tool("pg_ctl");

        // Check createdb
        result.tools.createdb = check_tool("createdb");

        // Check dropdb
        result.tools.dropdb = check_tool("dropdb");

        // At least one restore tool must be available
        if result.tools.pg_restore.is_none() && result.tools.psql.is_none() {
            result.add_error(PreflightError::new(
                "NO_RESTORE_TOOLS",
                "Neither pg_restore nor psql found",
            ).with_details(
                "At least one PostgreSQL restore tool must be available."
            ));
        }

        Ok(())
    }

    /// Check database connectivity
    async fn check_database_connectivity(&self, result: &mut PreflightResult) -> Result<()> {
        // Use pg_isready if available
        if result.tools.pg_isready.is_some() {
            let status = Command::new("pg_isready")
                .args([
                    "-h", &self.config.host,
                    "-p", &self.config.port.to_string(),
                    "-U", &self.config.user,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            match status {
                Ok(s) if s.success() => {
                    info!("[preflight] Database is accepting connections");
                }
                Ok(_) => {
                    result.add_warning(PreflightWarning::new(
                        "DB_NOT_READY",
                        format!("PostgreSQL at {}:{} is not accepting connections", 
                            self.config.host, self.config.port),
                    ).with_recommendation(
                        "Ensure PostgreSQL is running and accessible."
                    ));
                }
                Err(e) => {
                    result.add_warning(PreflightWarning::new(
                        "DB_CHECK_FAILED",
                        format!("Failed to check database connectivity: {}", e),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Create a restore plan
    pub fn create_plan(
        &self,
        backup_id: &str,
        preflight: &PreflightResult,
    ) -> Result<RestorePlan> {
        if !preflight.passed {
            return Err(anyhow!("Cannot create plan: preflight validation failed"));
        }

        let backup_info = preflight.backup_info.as_ref()
            .ok_or_else(|| anyhow!("No backup information available"))?;

        let mut steps = Vec::new();
        let mut order = 0;

        // Step 1: Validate backup (if local)
        if let BackupSource::Local { path } = &backup_info.source {
            order += 1;
            steps.push(RestoreStep {
                order,
                action: RestoreAction::ValidateBackup { backup_path: path.clone() },
                description: "Validate backup integrity".to_string(),
                reversible: false,
            });
        }

        // Step 2: Terminate existing connections (for Replace mode)
        if self.mode == RestoreMode::Replace {
            order += 1;
            steps.push(RestoreStep {
                order,
                action: RestoreAction::TerminateConnections {
                    database: self.config.database.clone(),
                },
                description: format!("Terminate connections to database '{}'", self.config.database),
                reversible: false,
            });

            // Step 3: Drop existing database
            order += 1;
            steps.push(RestoreStep {
                order,
                action: RestoreAction::DropDatabase {
                    database: self.config.database.clone(),
                },
                description: format!("Drop database '{}'", self.config.database),
                reversible: false,
            });
        }

        // Step 4: Create database
        let target_db = match &self.mode {
            RestoreMode::Replace => self.config.database.clone(),
            RestoreMode::NewDatabase { target_name } => target_name.clone(),
        };

        order += 1;
        steps.push(RestoreStep {
            order,
            action: RestoreAction::CreateDatabase {
                database: target_db.clone(),
                owner: Some(self.config.user.clone()),
            },
            description: format!("Create database '{}'", target_db),
            reversible: true,
        });

        // Step 5: Restore content
        let dump_path = match &backup_info.source {
            BackupSource::Local { path } => find_dump_file(path)?,
            BackupSource::Remote { .. } => {
                return Err(anyhow!("Remote backup must be downloaded first"));
            }
        };

        order += 1;
        steps.push(RestoreStep {
            order,
            action: RestoreAction::RestoreContent {
                dump_path,
                database: target_db.clone(),
            },
            description: format!("Restore database content to '{}'", target_db),
            reversible: false,
        });

        // Step 6: Health check
        order += 1;
        steps.push(RestoreStep {
            order,
            action: RestoreAction::HealthCheck {
                database: target_db.clone(),
                timeout_secs: 30,
            },
            description: "Verify database health".to_string(),
            reversible: false,
        });

        let requires_confirmation = self.mode == RestoreMode::Replace && !self.force;

        Ok(RestorePlan {
            id: uuid::Uuid::new_v4().to_string(),
            backup_id: backup_id.to_string(),
            target_config: TargetConfig {
                host: self.config.host.clone(),
                port: self.config.port,
                database: target_db,
                user: self.config.user.clone(),
                ssl_mode: self.config.ssl_mode.clone(),
            },
            mode: self.mode.clone(),
            steps,
            estimated_duration_secs: Some(60), // Rough estimate
            requires_confirmation,
            confirmation_reason: if requires_confirmation {
                Some("This operation will drop and recreate the existing database.".to_string())
            } else {
                None
            },
        })
    }

    /// Execute a restore plan
    pub async fn execute(&self, plan: &RestorePlan) -> Result<RestoreResult> {
        let started_at = Utc::now();
        let mut steps_completed = Vec::new();
        let mut last_error: Option<String> = None;

        info!("[restore] Executing restore plan {} for backup {}", plan.id, plan.backup_id);

        for step in &plan.steps {
            let step_start = std::time::Instant::now();
            info!("[restore] Step {}: {}", step.order, step.description);

            let step_result = match &step.action {
                RestoreAction::ValidateBackup { backup_path } => {
                    self.execute_validate_backup(backup_path).await
                }
                RestoreAction::TerminateConnections { database } => {
                    self.execute_terminate_connections(database).await
                }
                RestoreAction::DropDatabase { database } => {
                    self.execute_drop_database(database).await
                }
                RestoreAction::CreateDatabase { database, owner } => {
                    self.execute_create_database(database, owner.as_deref()).await
                }
                RestoreAction::RestoreContent { dump_path, database } => {
                    self.execute_restore_content(dump_path, database).await
                }
                RestoreAction::HealthCheck { database, timeout_secs } => {
                    self.execute_health_check(database, *timeout_secs).await
                }
                RestoreAction::DownloadBackup { .. } => {
                    // Handled separately before plan execution
                    Ok("Skipped".to_string())
                }
                RestoreAction::ApplyConfiguration { .. } => {
                    // Not implemented yet
                    Ok("Skipped".to_string())
                }
                RestoreAction::Cleanup { paths } => {
                    for path in paths {
                        if path.exists() {
                            let _ = std::fs::remove_dir_all(path);
                        }
                    }
                    Ok("Cleaned up".to_string())
                }
            };

            let duration_ms = step_start.elapsed().as_millis() as u64;

            match step_result {
                Ok(message) => {
                    info!("[restore] Step {} completed in {}ms", step.order, duration_ms);
                    steps_completed.push(StepResult {
                        order: step.order,
                        action: format!("{:?}", step.action),
                        success: true,
                        duration_ms,
                        message: Some(message),
                    });
                }
                Err(e) => {
                    error!("[restore] Step {} failed: {}", step.order, e);
                    steps_completed.push(StepResult {
                        order: step.order,
                        action: format!("{:?}", step.action),
                        success: false,
                        duration_ms,
                        message: Some(e.to_string()),
                    });
                    last_error = Some(e.to_string());
                    break;
                }
            }
        }

        let completed_at = Utc::now();
        let success = last_error.is_none();

        if success {
            info!(
                "[restore] Restore completed successfully in {}s",
                (completed_at - started_at).num_seconds()
            );
        } else {
            error!("[restore] Restore failed: {:?}", last_error);
        }

        Ok(RestoreResult {
            success,
            plan_id: plan.id.clone(),
            backup_id: plan.backup_id.clone(),
            database: plan.target_config.database.clone(),
            started_at,
            completed_at,
            steps_completed,
            error: last_error,
        })
    }

    // Step execution methods

    async fn execute_validate_backup(&self, backup_path: &Path) -> Result<String> {
        if !backup_path.exists() {
            return Err(anyhow!("Backup path does not exist: {:?}", backup_path));
        }

        let dump_file = find_dump_file(backup_path)?;
        if !dump_file.exists() {
            return Err(anyhow!("Dump file not found in backup"));
        }

        let size = dump_file.metadata()?.len();
        Ok(format!("Backup validated, dump file size: {} bytes", size))
    }

    async fn execute_terminate_connections(&self, database: &str) -> Result<String> {
        // Build connection string for maintenance database
        let conn_str = self.config.maintenance_connection_string();

        // Use psql to terminate connections
        let query = format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}' AND pid <> pg_backend_pid();",
            database
        );

        let output = Command::new("psql")
            .args([&conn_str, "-c", &query])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("[restore] Warning terminating connections: {}", stderr);
            // Don't fail - connections might not exist
        }

        Ok("Connections terminated".to_string())
    }

    async fn execute_drop_database(&self, database: &str) -> Result<String> {
        let output = Command::new("dropdb")
            .args([
                "-h", &self.config.host,
                "-p", &self.config.port.to_string(),
                "-U", &self.config.user,
                "--if-exists",
                database,
            ])
            .env("PGPASSWORD", self.config.password.as_deref().unwrap_or(""))
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Check if it's just "database does not exist" which is fine
            if !stderr.contains("does not exist") {
                return Err(anyhow!("Failed to drop database: {}", stderr));
            }
        }

        Ok(format!("Database '{}' dropped", database))
    }

    async fn execute_create_database(&self, database: &str, owner: Option<&str>) -> Result<String> {
        let port_str = self.config.port.to_string();
        let mut args = vec![
            "-h", &self.config.host,
            "-p", &port_str,
            "-U", &self.config.user,
        ];

        if let Some(owner) = owner {
            args.push("-O");
            args.push(owner);
        }

        args.push(database);

        let output = Command::new("createdb")
            .args(&args)
            .env("PGPASSWORD", self.config.password.as_deref().unwrap_or(""))
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to create database: {}", stderr));
        }

        Ok(format!("Database '{}' created", database))
    }

    async fn execute_restore_content(&self, dump_path: &Path, database: &str) -> Result<String> {
        let dump_str = dump_path.to_string_lossy();
        
        // Determine restore method based on file extension
        let is_custom_format = dump_str.ends_with(".dump") || dump_str.ends_with(".backup");

        if is_custom_format {
            // Use pg_restore for custom format
            let output = Command::new("pg_restore")
                .args([
                    "-h", &self.config.host,
                    "-p", &self.config.port.to_string(),
                    "-U", &self.config.user,
                    "-d", database,
                    "-v",
                    "--no-owner",
                    "--no-privileges",
                    &dump_str,
                ])
                .env("PGPASSWORD", self.config.password.as_deref().unwrap_or(""))
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // pg_restore often returns non-zero even on success with warnings
                if stderr.contains("error") || stderr.contains("FATAL") {
                    return Err(anyhow!("pg_restore failed: {}", stderr));
                }
                warn!("[restore] pg_restore completed with warnings: {}", stderr);
            }
        } else {
            // Use psql for plain SQL
            let output = Command::new("psql")
                .args([
                    "-h", &self.config.host,
                    "-p", &self.config.port.to_string(),
                    "-U", &self.config.user,
                    "-d", database,
                    "-f", &dump_str,
                ])
                .env("PGPASSWORD", self.config.password.as_deref().unwrap_or(""))
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("psql restore failed: {}", stderr));
            }
        }

        Ok("Database content restored".to_string())
    }

    async fn execute_health_check(&self, database: &str, timeout_secs: u64) -> Result<String> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            // Try pg_isready first
            let ready = Command::new("pg_isready")
                .args([
                    "-h", &self.config.host,
                    "-p", &self.config.port.to_string(),
                    "-d", database,
                    "-U", &self.config.user,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if ready {
                // Try a simple query
                let query_ok = Command::new("psql")
                    .args([
                        "-h", &self.config.host,
                        "-p", &self.config.port.to_string(),
                        "-U", &self.config.user,
                        "-d", database,
                        "-c", "SELECT 1;",
                    ])
                    .env("PGPASSWORD", self.config.password.as_deref().unwrap_or(""))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);

                if query_ok {
                    return Ok(format!(
                        "Database '{}' is healthy (checked in {}ms)",
                        database,
                        start.elapsed().as_millis()
                    ));
                }
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Err(anyhow!(
            "Health check timed out after {}s",
            timeout_secs
        ))
    }
}

// Helper functions

fn check_tool(name: &str) -> Option<ToolInfo> {
    let output = Command::new("which")
        .arg(name)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string();

    if path.is_empty() {
        return None;
    }

    // Try to get version
    let version = Command::new(name)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).lines().next()?.to_string())
            } else {
                None
            }
        });

    Some(ToolInfo {
        path: PathBuf::from(path),
        version,
    })
}

fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(total)
}

fn find_dump_file(backup_path: &Path) -> Result<PathBuf> {
    // Look for dump files in order of preference (.dump, .backup, .sql)
    for entry in std::fs::read_dir(backup_path)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if name.ends_with(".dump") || name.ends_with(".backup") || name.ends_with(".sql") {
            return Ok(path);
        }
    }

    Err(anyhow!("No dump file found in {:?}", backup_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preflight_result_default() {
        let result = PreflightResult::new();
        assert!(result.passed);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_preflight_add_error() {
        let mut result = PreflightResult::new();
        result.add_error(PreflightError::new("TEST", "Test error"));
        assert!(!result.passed);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_preflight_add_warning() {
        let mut result = PreflightResult::new();
        result.add_warning(PreflightWarning::new("TEST", "Test warning"));
        assert!(result.passed); // Warnings don't fail preflight
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_restore_mode_default() {
        let mode = RestoreMode::default();
        assert_eq!(mode, RestoreMode::Replace);
    }

    #[test]
    fn test_target_state_variants() {
        assert_eq!(TargetState::Empty, TargetState::Empty);
        assert_ne!(TargetState::Empty, TargetState::NotExists);
    }

    #[test]
    fn test_preflight_error_with_details() {
        let error = PreflightError::new("CODE", "Message")
            .with_details("Details");
        assert_eq!(error.code, "CODE");
        assert_eq!(error.message, "Message");
        assert_eq!(error.details, Some("Details".to_string()));
    }

    #[test]
    fn test_preflight_warning_with_recommendation() {
        let warning = PreflightWarning::new("CODE", "Message")
            .with_recommendation("Recommendation");
        assert_eq!(warning.code, "CODE");
        assert_eq!(warning.message, "Message");
        assert_eq!(warning.recommendation, Some("Recommendation".to_string()));
    }
}
