//! Full Restore CLI Command Implementation
//!
//! Implements the `warden postgres full-restore` command for restoring
//! PostgreSQL backups to replacement instances.

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use super::{create_storage_provider, SshOptions, StorageOptions};
use crate::common::PostgresConfig;
use crate::restore::full_restore::{
    FullRestoreManager, PreflightResult, RestoreMode, RestorePlan, RestoreResult,
};
use crate::tunnel_keeper::TunnelKeeper;

/// Options for the full restore command
#[derive(Debug, Clone)]
pub struct FullRestoreOptions {
    /// Backup identifier (UUID or path)
    pub backup_id: String,
    /// Target PostgreSQL host
    pub host: String,
    /// Target PostgreSQL port
    pub port: u16,
    /// Target database name
    pub database: String,
    /// PostgreSQL user
    pub user: String,
    /// PostgreSQL password
    pub password: Option<String>,
    /// SSL mode
    pub ssl_mode: Option<String>,
    /// Local backup directory
    pub backup_dir: PathBuf,
    /// Restore mode: replace existing or create new database
    pub mode: RestoreMode,
    /// Skip confirmation prompts
    pub yes: bool,
    /// Dry run - show plan without executing
    pub dry_run: bool,
    /// Output format (table, json)
    pub format: String,
    /// SSH options for remote target
    pub ssh: SshOptions,
    /// Storage options for remote backup
    pub storage: StorageOptions,
}

/// Execute the full restore command
#[allow(clippy::too_many_arguments)]
pub async fn full_restore(
    backup_id: String,
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    target_database: Option<String>,
    yes: bool,
    dry_run: bool,
    format: String,
    ssh: SshOptions,
    storage: StorageOptions,
) -> Result<()> {
    info!("[full-restore] Starting full restore operation");
    info!("[full-restore] Backup ID: {}", backup_id);
    info!("[full-restore] Target: {}:{}/{}", host, port, database);

    // Determine restore mode
    let mode = match target_database {
        Some(target_name) if target_name != database => {
            info!(
                "[full-restore] Mode: Restore to new database '{}'",
                target_name
            );
            RestoreMode::NewDatabase { target_name }
        }
        _ => {
            info!("[full-restore] Mode: Replace existing database");
            RestoreMode::Replace
        }
    };

    // Build effective config (adjust for SSH tunnel if needed)
    let effective_host = if ssh.host.is_some() {
        "localhost".to_string()
    } else {
        host.clone()
    };
    let effective_port = if ssh.host.is_some() {
        ssh.local_port.unwrap_or(6969)
    } else {
        port
    };

    let config = PostgresConfig {
        host: effective_host,
        port: effective_port,
        database: database.clone(),
        user: user.clone(),
        password: password.clone(),
        ssl_mode: ssl_mode.clone(),
        maintenance_db: Some("postgres".to_string()),
        ssh_host: ssh.host.clone(),
        ssh_user: ssh.user.clone(),
        ssh_port: ssh.port,
        ssh_password: ssh.password.clone(),
        ssh_key_path: ssh.key_path.clone(),
        ssh_local_port: ssh.local_port,
        ssh_remote_port: ssh.remote_port,
    };

    // Setup SSH tunnel if needed
    if config.ssh_host.is_some() {
        info!("[full-restore] Setting up SSH tunnel...");
        let keeper_instance = TunnelKeeper::instance().await;
        let mut keeper = keeper_instance.lock().await;
        if let Err(e) = keeper.setup(&config).await {
            error!("[full-restore] Failed to setup SSH tunnel: {}", e);
            return Err(anyhow!("SSH tunnel setup failed: {}", e));
        }
        info!("[full-restore] SSH tunnel established");
    }

    // Download backup from remote storage if needed
    let local_backup_id = if storage.remote_storage {
        info!("[full-restore] Downloading backup from remote storage...");
        download_backup_from_storage(&storage, &backup_id, &backup_dir).await?
    } else {
        backup_id.clone()
    };

    // Create restore manager
    let manager = FullRestoreManager::new(config.clone(), backup_dir.clone())
        .with_mode(mode.clone())
        .with_force(yes);

    // Run preflight validation
    info!("[full-restore] Running preflight validation...");
    let preflight = manager.preflight(&local_backup_id, None).await?;

    // Display preflight results
    display_preflight_results(&preflight, &format)?;

    if !preflight.passed {
        error!("[full-restore] Preflight validation failed");
        close_ssh_tunnel(&config).await;
        return Err(anyhow!("Preflight validation failed. See errors above."));
    }

    // Create restore plan
    info!("[full-restore] Creating restore plan...");
    let plan = manager.create_plan(&local_backup_id, &preflight)?;

    // Display plan
    display_restore_plan(&plan, &format)?;

    // If dry run, stop here
    if dry_run {
        info!("[full-restore] Dry run complete. No changes made.");
        close_ssh_tunnel(&config).await;
        return Ok(());
    }

    // Confirm if required
    if plan.requires_confirmation && !yes && !confirm_restore(&plan)? {
        info!("[full-restore] Restore cancelled by user");
        close_ssh_tunnel(&config).await;
        return Ok(());
    }

    // Execute restore
    info!("[full-restore] Executing restore...");
    let result = manager.execute(&plan).await?;

    // Display results
    display_restore_result(&result, &format)?;

    // Close SSH tunnel
    close_ssh_tunnel(&config).await;

    if result.success {
        info!("[full-restore] Restore completed successfully");
        Ok(())
    } else {
        Err(anyhow!("Restore failed: {:?}", result.error))
    }
}

/// Download backup from remote storage
async fn download_backup_from_storage(
    storage: &StorageOptions,
    backup_id: &str,
    backup_dir: &Path,
) -> Result<String> {
    let storage_instance = create_storage_provider(storage)
        .await?
        .ok_or_else(|| anyhow!("Storage provider not configured"))?;

    let local_path = backup_dir.join(backup_id);
    if !local_path.exists() {
        std::fs::create_dir_all(&local_path)?;
    }

    info!(
        "[full-restore] Downloading backup {} to {:?}",
        backup_id, local_path
    );

    storage_instance
        .download_backup(backup_id, &local_path)
        .await
        .map_err(|e| anyhow!("Failed to download backup: {}", e))?;

    info!("[full-restore] Backup downloaded successfully");
    Ok(backup_id.to_string())
}

/// Display preflight validation results
fn display_preflight_results(preflight: &PreflightResult, format: &str) -> Result<()> {
    if format == "json" {
        let json = serde_json::to_string_pretty(preflight)?;
        println!("{}", json);
        return Ok(());
    }

    println!("\n=== Preflight Validation ===");
    println!(
        "Status: {}",
        if preflight.passed {
            "✓ PASSED"
        } else {
            "✗ FAILED"
        }
    );

    if let Some(backup) = &preflight.backup_info {
        println!("\n📦 Backup Information:");
        println!("   ID: {}", backup.id);
        println!("   Type: {}", backup.backup_type);
        if let Some(db) = &backup.database {
            println!("   Database: {}", db);
        }
        println!(
            "   Size: {} bytes ({:.2} MB)",
            backup.size_bytes,
            backup.size_bytes as f64 / 1024.0 / 1024.0
        );
        println!("   Created: {}", backup.created_at);
        if let Some(version) = &backup.server_version {
            println!("   Server Version: {}", version);
        }
    }

    println!("\n🎯 Target State: {:?}", preflight.target_state);

    println!("\n🔧 Tools:");
    if let Some(tool) = &preflight.tools.pg_restore {
        println!("   pg_restore: ✓ {:?}", tool.path);
    } else {
        println!("   pg_restore: ✗ not found");
    }
    if let Some(tool) = &preflight.tools.psql {
        println!("   psql: ✓ {:?}", tool.path);
    } else {
        println!("   psql: ✗ not found");
    }
    if let Some(tool) = &preflight.tools.createdb {
        println!("   createdb: ✓ {:?}", tool.path);
    } else {
        println!("   createdb: ✗ not found");
    }
    if let Some(tool) = &preflight.tools.dropdb {
        println!("   dropdb: ✓ {:?}", tool.path);
    } else {
        println!("   dropdb: ✗ not found");
    }

    if !preflight.errors.is_empty() {
        println!("\n❌ Errors:");
        for error in &preflight.errors {
            println!("   [{}] {}", error.code, error.message);
            if let Some(details) = &error.details {
                println!("      {}", details);
            }
        }
    }

    if !preflight.warnings.is_empty() {
        println!("\n⚠️  Warnings:");
        for warning in &preflight.warnings {
            println!("   [{}] {}", warning.code, warning.message);
            if let Some(rec) = &warning.recommendation {
                println!("      Recommendation: {}", rec);
            }
        }
    }

    println!();
    Ok(())
}

/// Display restore plan
fn display_restore_plan(plan: &RestorePlan, format: &str) -> Result<()> {
    if format == "json" {
        let json = serde_json::to_string_pretty(plan)?;
        println!("{}", json);
        return Ok(());
    }

    println!("\n=== Restore Plan ===");
    println!("Plan ID: {}", plan.id);
    println!("Backup ID: {}", plan.backup_id);
    println!(
        "Target: {}:{}/{}",
        plan.target_config.host, plan.target_config.port, plan.target_config.database
    );
    println!("Mode: {:?}", plan.mode);

    if let Some(duration) = plan.estimated_duration_secs {
        println!("Estimated Duration: ~{}s", duration);
    }

    println!("\n📋 Steps:");
    for step in &plan.steps {
        let reversible = if step.reversible { " (reversible)" } else { "" };
        println!("   {}. {}{}", step.order, step.description, reversible);
    }

    if plan.requires_confirmation {
        println!(
            "\n⚠️  Confirmation Required: {}",
            plan.confirmation_reason
                .as_deref()
                .unwrap_or("Destructive operation")
        );
    }

    println!();
    Ok(())
}

/// Display restore result
fn display_restore_result(result: &RestoreResult, format: &str) -> Result<()> {
    if format == "json" {
        let json = serde_json::to_string_pretty(result)?;
        println!("{}", json);
        return Ok(());
    }

    println!("\n=== Restore Result ===");
    println!(
        "Status: {}",
        if result.success {
            "✓ SUCCESS"
        } else {
            "✗ FAILED"
        }
    );
    println!("Plan ID: {}", result.plan_id);
    println!("Backup ID: {}", result.backup_id);
    println!("Database: {}", result.database);
    println!("Started: {}", result.started_at);
    println!("Completed: {}", result.completed_at);
    println!(
        "Duration: {}s",
        (result.completed_at - result.started_at).num_seconds()
    );

    println!("\n📋 Steps Completed:");
    for step in &result.steps_completed {
        let status = if step.success { "✓" } else { "✗" };
        println!("   {} Step {}: {}ms", status, step.order, step.duration_ms);
        if let Some(msg) = &step.message {
            println!("      {}", msg);
        }
    }

    if let Some(error) = &result.error {
        println!("\n❌ Error: {}", error);
    }

    println!();
    Ok(())
}

/// Prompt user for confirmation
fn confirm_restore(plan: &RestorePlan) -> Result<bool> {
    println!("\n⚠️  WARNING: This operation will modify the database.");
    if let Some(reason) = &plan.confirmation_reason {
        println!("   {}", reason);
    }
    println!();
    print!("Do you want to proceed? [y/N]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let confirmed = input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes";
    Ok(confirmed)
}

/// Close SSH tunnel if active
async fn close_ssh_tunnel(config: &PostgresConfig) {
    if config.ssh_host.is_some() {
        info!("[full-restore] Closing SSH tunnel...");
        let keeper_instance = TunnelKeeper::instance().await;
        let is_active = {
            let keeper = keeper_instance.lock().await;
            keeper.is_active.load(Ordering::SeqCst)
        };
        if is_active {
            let mut keeper = keeper_instance.lock().await;
            if let Err(e) = keeper.close().await {
                warn!("[full-restore] Error closing SSH tunnel: {}", e);
            } else {
                info!("[full-restore] SSH tunnel closed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_restore_mode_from_target_database() {
        // Same database = Replace mode
        let mode = match Some("mydb".to_string()) {
            Some(target) if target != "mydb" => RestoreMode::NewDatabase {
                target_name: target,
            },
            _ => RestoreMode::Replace,
        };
        assert_eq!(mode, RestoreMode::Replace);

        // Different database = NewDatabase mode
        let mode = match Some("newdb".to_string()) {
            Some(target) if target != "mydb" => RestoreMode::NewDatabase {
                target_name: target,
            },
            _ => RestoreMode::Replace,
        };
        assert!(matches!(mode, RestoreMode::NewDatabase { target_name } if target_name == "newdb"));
    }
}
