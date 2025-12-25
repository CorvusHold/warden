//! CLI command handlers for HA orchestration.
//!
//! This module provides the command execution logic for:
//! - `ha-switchover`: Planned primary role transfer
//! - `ha-failover`: Emergency replica promotion
//! - `ha-clone-node`: Create new replica from backup

use anyhow::{anyhow, Result};
use chrono::DateTime;
use log::info;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::ha::{
    format_plan, format_result, CloneNodeOptions, CloneNodeOrchestrator, FailoverOptions,
    FailoverOrchestrator, SwitchoverOptions, SwitchoverOrchestrator,
};

/// Execute the ha-switchover command.
#[allow(clippy::too_many_arguments)]
pub async fn execute_ha_switchover(
    cluster: String,
    from_node: String,
    to_node: String,
    config: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    max_lag_bytes: u64,
    catchup_timeout: u64,
    pg_user: String,
    pg_password: Option<String>,
    database: String,
    target_data_dir: Option<String>,
    format: String,
) -> Result<()> {
    info!(
        "[ha-switchover] Starting switchover from {} to {} in cluster {}",
        from_node, to_node, cluster
    );

    let options = SwitchoverOptions {
        cluster_id: cluster,
        from_node_id: from_node,
        to_node_id: to_node,
        config_path: config,
        dry_run,
        yes,
        max_lag_bytes,
        catchup_timeout_secs: catchup_timeout,
        pg_user,
        pg_password,
        database,
        target_data_dir,
    };

    let orchestrator = SwitchoverOrchestrator::new(options).map_err(|e| anyhow!("{}", e))?;

    // Create the plan
    let mut plan = orchestrator.plan().map_err(|e| anyhow!("{}", e))?;

    // Display the plan
    println!("{}", format_plan(&plan, &format));

    // If not dry-run and not --yes, ask for confirmation
    if !dry_run && !plan.dry_run {
        if !yes && plan.has_destructive_steps() {
            print!("\nDo you want to proceed with this switchover? [y/N] ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Switchover cancelled.");
                return Ok(());
            }
        }

        // Execute the plan
        let result = orchestrator
            .execute(&mut plan)
            .await
            .map_err(|e| anyhow!("{}", e))?;

        println!("{}", format_result(&result, &format));

        if !result.success {
            return Err(anyhow!("Switchover failed: {}", result.message));
        }
    } else {
        println!("\n[DRY-RUN] No changes were made.");
    }

    Ok(())
}

/// Execute the ha-failover command.
#[allow(clippy::too_many_arguments)]
pub async fn execute_ha_failover(
    cluster: String,
    to_node: String,
    target_time: Option<String>,
    config: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    force: bool,
    pg_user: String,
    pg_password: Option<String>,
    database: String,
    target_data_dir: Option<String>,
    backup_dir: PathBuf,
    format: String,
) -> Result<()> {
    info!(
        "[ha-failover] Starting failover to {} in cluster {}",
        to_node, cluster
    );

    // Parse target time if provided
    let parsed_target_time = if let Some(ref time_str) = target_time {
        Some(
            DateTime::parse_from_rfc3339(time_str)
                .map_err(|e| anyhow!("Invalid target time format: {}. Use RFC3339 format (e.g., 2025-01-15T10:30:00Z)", e))?
                .with_timezone(&chrono::Utc),
        )
    } else {
        None
    };

    let options = FailoverOptions {
        cluster_id: cluster,
        to_node_id: to_node,
        target_time: parsed_target_time,
        config_path: config,
        dry_run,
        yes,
        force,
        pg_user,
        pg_password,
        database,
        target_data_dir,
        backup_dir: Some(backup_dir),
    };

    let orchestrator = FailoverOrchestrator::new(options).map_err(|e| anyhow!("{}", e))?;

    // Create the plan
    let mut plan = orchestrator.plan().map_err(|e| anyhow!("{}", e))?;

    // Display the plan
    println!("{}", format_plan(&plan, &format));

    // If not dry-run and not --yes, ask for confirmation
    if !dry_run && !plan.dry_run {
        if !yes {
            println!("\n⚠️  WARNING: This is an emergency failover operation.");
            println!("    Data loss may occur for transactions not yet replicated.");
            print!("\nDo you want to proceed with this failover? [y/N] ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Failover cancelled.");
                return Ok(());
            }
        }

        // Execute the plan
        let result = orchestrator
            .execute(&mut plan)
            .await
            .map_err(|e| anyhow!("{}", e))?;

        println!("{}", format_result(&result, &format));

        if !result.success {
            return Err(anyhow!("Failover failed: {}", result.message));
        }
    } else {
        println!("\n[DRY-RUN] No changes were made.");
    }

    Ok(())
}

/// Execute the ha-clone-node command.
#[allow(clippy::too_many_arguments)]
pub async fn execute_ha_clone_node(
    cluster: String,
    source_node: String,
    target_node: String,
    backup_id: Option<String>,
    target_time: Option<String>,
    target_dir: PathBuf,
    config: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    backup_dir: PathBuf,
    pg_user: String,
    pg_password: Option<String>,
    database: String,
    remote_storage: bool,
    storage_bucket: Option<String>,
    storage_endpoint: Option<String>,
    storage_region: Option<String>,
    storage_access_key: Option<String>,
    storage_secret_key: Option<String>,
    format: String,
) -> Result<()> {
    info!(
        "[ha-clone-node] Starting clone from {} to {} in cluster {}",
        source_node, target_node, cluster
    );

    // Parse target time if provided
    let parsed_target_time = if let Some(ref time_str) = target_time {
        Some(
            DateTime::parse_from_rfc3339(time_str)
                .map_err(|e| anyhow!("Invalid target time format: {}. Use RFC3339 format (e.g., 2025-01-15T10:30:00Z)", e))?
                .with_timezone(&chrono::Utc),
        )
    } else {
        None
    };

    let options = CloneNodeOptions {
        cluster_id: cluster,
        source_node_id: source_node,
        target_node_id: target_node,
        backup_id,
        target_time: parsed_target_time,
        target_dir,
        config_path: config,
        dry_run,
        yes,
        backup_dir,
        pg_user,
        pg_password,
        database,
        remote_storage,
        storage_bucket,
        storage_endpoint,
        storage_region,
        storage_access_key,
        storage_secret_key,
    };

    let orchestrator = CloneNodeOrchestrator::new(options).map_err(|e| anyhow!("{}", e))?;

    // Create the plan
    let mut plan = orchestrator.plan().map_err(|e| anyhow!("{}", e))?;

    // Display the plan
    println!("{}", format_plan(&plan, &format));

    // If not dry-run and not --yes, ask for confirmation
    if !dry_run && !plan.dry_run {
        if !yes && plan.has_destructive_steps() {
            print!("\nDo you want to proceed with this clone operation? [y/N] ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Clone operation cancelled.");
                return Ok(());
            }
        }

        // Execute the plan
        let result = orchestrator
            .execute(&mut plan)
            .await
            .map_err(|e| anyhow!("{}", e))?;

        println!("{}", format_result(&result, &format));

        if !result.success {
            return Err(anyhow!("Clone operation failed: {}", result.message));
        }
    } else {
        println!("\n[DRY-RUN] No changes were made.");
    }

    Ok(())
}
