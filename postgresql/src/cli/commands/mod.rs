use crate::{PostgresConfig, PostgresManager};
use anyhow::Result;
use std::path::PathBuf;
use uuid::Uuid;

pub async fn full_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    println!("Performing full backup...");
    let backup = manager
        .full_backup()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Full backup completed: {}", backup.id);
    Ok(())
}

pub async fn incremental_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    println!("Performing incremental backup...");
    let backup = manager
        .incremental_backup()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Incremental backup completed: {}", backup.id);
    Ok(())
}

pub async fn snapshot_backup(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let mut manager = PostgresManager::new(config, backup_dir)?;
    println!("Performing snapshot backup...");
    let backup = manager
        .snapshot_backup()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Snapshot backup completed: {}", backup.id);
    Ok(())
}

pub async fn list_backups(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    println!("All backups:");
    for backup in manager.list_backups() {
        println!(
            "Backup ID: {}, Type: {:?}, Status: {:?}, Time: {}",
            backup.id, backup.backup_type, backup.status, backup.start_time
        );
    }
    Ok(())
}

pub async fn restore_full(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    backup_id: String,
    target_dir: PathBuf,
    container_id: Option<String>,
    container_type: Option<String>,
    auto_restart: bool,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    println!(
        "Restoring from full backup {} to {:?}...",
        backup_id, target_dir
    );
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_full_backup(&backup_id, target_dir)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

pub async fn restore_incremental(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    full_backup_id: String,
    target_dir: PathBuf,
    container_id: Option<String>,
    container_type: Option<String>,
    auto_restart: bool,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    println!(
        "Restoring with incremental backups from {} to {:?}...",
        full_backup_id, target_dir
    );
    let full_backup_id = Uuid::parse_str(&full_backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_incremental_backup(&full_backup_id, target_dir)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

pub async fn restore_point_in_time(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    full_backup_id: String,
    target_dir: PathBuf,
    target_time: String,
    container_id: Option<String>,
    container_type: Option<String>,
    auto_restart: bool,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    // Parse target time
    let target_time = chrono::DateTime::parse_from_str(&target_time, "%Y-%m-%dT%H:%M:%S%z")
        .map_err(|e| anyhow::anyhow!("Invalid target time format: {}", e))?
        .with_timezone(&chrono::Utc);

    println!(
        "Restoring to point in time {} from {} to {:?}...",
        target_time, full_backup_id, target_dir
    );
    let full_backup_id = Uuid::parse_str(&full_backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_point_in_time(&full_backup_id, target_dir, target_time)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

pub async fn restore_snapshot(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    backup_id: String,
    target_dir: PathBuf,
    container_id: Option<String>,
    container_type: Option<String>,
    auto_restart: bool,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    println!(
        "Restoring from snapshot backup {} to {:?}...",
        backup_id, target_dir
    );
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let restore = manager
        .restore_snapshot_backup(&backup_id, target_dir)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    println!("Restore completed: {}", restore.id);

    // Handle PostgreSQL restart if requested
    if auto_restart {
        restart_postgresql(container_id, container_type).await?;
    }

    Ok(())
}

/// Restart PostgreSQL after a restore operation in container or local environments
async fn restart_postgresql(
    container_id: Option<String>,
    container_type: Option<String>,
) -> Result<()> {
    match (container_id, container_type.as_deref()) {
        (Some(id), Some("docker")) => {
            println!("Restarting PostgreSQL in Docker container {}...", id);
            // Execute Docker command to restart PostgreSQL
            let output = std::process::Command::new("docker")
                .args([
                    "exec",
                    &id,
                    "pg_ctl",
                    "restart",
                    "-D",
                    "/var/lib/postgresql/data",
                ])
                .output()
                .map_err(|e| {
                    anyhow::anyhow!("Failed to restart PostgreSQL in Docker container: {}", e)
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "Failed to restart PostgreSQL in Docker container: {}",
                    error
                ));
            }

            println!("PostgreSQL successfully restarted in Docker container");
        }
        (Some(id), Some("kubernetes")) => {
            println!("Restarting PostgreSQL in Kubernetes pod {}...", id);
            // Execute kubectl command to restart PostgreSQL
            let output = std::process::Command::new("kubectl")
                .args([
                    "exec",
                    &id,
                    "--",
                    "pg_ctl",
                    "restart",
                    "-D",
                    "/var/lib/postgresql/data",
                ])
                .output()
                .map_err(|e| {
                    anyhow::anyhow!("Failed to restart PostgreSQL in Kubernetes pod: {}", e)
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "Failed to restart PostgreSQL in Kubernetes pod: {}",
                    error
                ));
            }

            println!("PostgreSQL successfully restarted in Kubernetes pod");
        }
        (Some(_), Some(invalid_type)) => {
            return Err(anyhow::anyhow!(
                "Invalid container type: {}. Supported types are 'docker' and 'kubernetes'",
                invalid_type
            ));
        }
        (Some(_), None) => {
            return Err(anyhow::anyhow!("Container ID provided but container type is missing. Please specify --container-type"));
        }
        (None, Some(_)) => {
            return Err(anyhow::anyhow!("Container type provided but container ID is missing. Please specify --container-id"));
        }
        (None, None) => {
            // Attempt to restart local PostgreSQL instance
            println!("Attempting to restart local PostgreSQL instance...");

            // Detect operating system
            let os = std::env::consts::OS;
            match os {
                "macos" => restart_postgresql_macos().await?,
                "linux" => restart_postgresql_linux().await?,
                _ => {
                    println!("Auto-restart not supported on {} operating system. Please restart PostgreSQL manually.", os);
                }
            }
        }
    }

    Ok(())
}

/// Restart PostgreSQL on macOS
async fn restart_postgresql_macos() -> Result<()> {
    // Try different methods for restarting PostgreSQL on macOS

    // Method 1: Using brew services (most common for Homebrew installations)
    if let Ok(output) = std::process::Command::new("brew")
        .args(["services", "restart", "postgresql"])
        .output()
    {
        if output.status.success() {
            println!("PostgreSQL successfully restarted using Homebrew services");
            return Ok(());
        }
    }

    // Method 2: Using pg_ctl directly (try common data directories)
    let data_dirs = [
        "/usr/local/var/postgres",
        "/opt/homebrew/var/postgres",
        "/usr/local/var/postgresql@14", // For specific versions
        "/usr/local/var/postgresql@13",
        "/usr/local/var/postgresql@12",
    ];

    for data_dir in data_dirs {
        if std::path::Path::new(data_dir).exists() {
            if let Ok(output) = std::process::Command::new("pg_ctl")
                .args(["restart", "-D", data_dir])
                .output()
            {
                if output.status.success() {
                    println!(
                        "PostgreSQL successfully restarted using pg_ctl with data directory: {}",
                        data_dir
                    );
                    return Ok(());
                }
            }
        }
    }

    // Method 3: Using launchctl for system installations
    if let Ok(output) = std::process::Command::new("launchctl")
        .args([
            "unload",
            "/Library/LaunchDaemons/org.postgresql.postgres.plist",
        ])
        .output()
    {
        if output.status.success() {
            if let Ok(output) = std::process::Command::new("launchctl")
                .args([
                    "load",
                    "/Library/LaunchDaemons/org.postgresql.postgres.plist",
                ])
                .output()
            {
                if output.status.success() {
                    println!("PostgreSQL successfully restarted using launchctl");
                    return Ok(());
                }
            }
        }
    }

    println!("Could not automatically restart PostgreSQL on macOS. Please restart it manually.");
    Ok(())
}

/// Restart PostgreSQL on Linux
async fn restart_postgresql_linux() -> Result<()> {
    // Try different methods for restarting PostgreSQL on Linux

    // Method 1: Using systemctl (most common on modern distros)
    if let Ok(output) = std::process::Command::new("systemctl")
        .args(["restart", "postgresql"])
        .output()
    {
        if output.status.success() {
            println!("PostgreSQL successfully restarted using systemctl");
            return Ok(());
        }
    }

    // Method 2: Try with specific version numbers
    for version in ["14", "13", "12", "11", "10", "9.6"] {
        let service_name = format!("postgresql-{}", version);
        if let Ok(output) = std::process::Command::new("systemctl")
            .args(["restart", &service_name])
            .output()
        {
            if output.status.success() {
                println!(
                    "PostgreSQL {} successfully restarted using systemctl",
                    version
                );
                return Ok(());
            }
        }
    }

    // Method 3: Using service command (older distros)
    if let Ok(output) = std::process::Command::new("service")
        .args(["postgresql", "restart"])
        .output()
    {
        if output.status.success() {
            println!("PostgreSQL successfully restarted using service command");
            return Ok(());
        }
    }

    // Method 4: Using pg_ctl directly with common data directories
    let data_dirs = [
        "/var/lib/postgresql/data",
        "/var/lib/postgresql/14/data",
        "/var/lib/postgresql/13/data",
        "/var/lib/postgresql/12/data",
    ];

    for data_dir in data_dirs {
        if std::path::Path::new(data_dir).exists() {
            if let Ok(output) = std::process::Command::new("pg_ctl")
                .args(["restart", "-D", data_dir])
                .output()
            {
                if output.status.success() {
                    println!(
                        "PostgreSQL successfully restarted using pg_ctl with data directory: {}",
                        data_dir
                    );
                    return Ok(());
                }
            }
        }
    }

    println!("Could not automatically restart PostgreSQL on Linux. Please restart it manually.");
    Ok(())
}

pub async fn list_snapshot_contents(
    host: String,
    port: u16,
    database: String,
    user: String,
    password: Option<String>,
    ssl_mode: Option<String>,
    backup_dir: PathBuf,
    backup_id: String,
) -> Result<()> {
    let config = PostgresConfig {
        host,
        port,
        database,
        user,
        password,
        ssl_mode,
    };
    let manager = PostgresManager::new(config, backup_dir)?;
    println!("Snapshot backup contents for {}:", backup_id);
    let backup_id = Uuid::parse_str(&backup_id).map_err(|e| anyhow::anyhow!(e))?;
    let contents = manager
        .list_snapshot_contents(&backup_id)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    for item in contents.split('\n').filter(|s| !s.is_empty()) {
        println!("{}", item);
    }
    Ok(())
}
