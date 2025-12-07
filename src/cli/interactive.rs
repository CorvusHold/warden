//! Interactive mode utilities for complex CLI operations
//!
//! Provides guided prompts for operations like PITR restore, HA switchover, etc.

use anyhow::{anyhow, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::io::{self, Write};

/// Theme for interactive prompts
pub fn get_theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

/// Prompt for confirmation with a custom message
pub fn confirm(message: &str, default: bool) -> Result<bool> {
    Confirm::with_theme(&get_theme())
        .with_prompt(message)
        .default(default)
        .interact()
        .map_err(|e| anyhow!("Failed to get confirmation: {}", e))
}

/// Prompt for text input with optional default
pub fn input(prompt: &str, default: Option<&str>) -> Result<String> {
    let theme = get_theme();
    
    if let Some(def) = default {
        Input::with_theme(&theme)
            .with_prompt(prompt)
            .default(def.to_string())
            .interact_text()
            .map_err(|e| anyhow!("Failed to get input: {}", e))
    } else {
        Input::with_theme(&theme)
            .with_prompt(prompt)
            .interact_text()
            .map_err(|e| anyhow!("Failed to get input: {}", e))
    }
}

/// Prompt for selection from a list of options
#[allow(dead_code)] // Public API for interactive prompts
pub fn select(prompt: &str, options: &[&str], default: usize) -> Result<usize> {
    Select::with_theme(&get_theme())
        .with_prompt(prompt)
        .items(options)
        .default(default)
        .interact()
        .map_err(|e| anyhow!("Failed to get selection: {}", e))
}

/// Display a summary box with key-value pairs
pub fn display_summary(title: &str, items: &[(&str, &str)]) {
    let max_key_len = items.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

    println!();
    println!("┌─ {} ─", title);
    println!("│");
    for (key, value) in items {
        println!("│  {:width$} : {}", key, value, width = max_key_len);
    }
    println!("│");
    println!("└─────────────────────────────────────────");
    println!();
}

/// Display a warning message
pub fn warn(message: &str) {
    eprintln!("\n⚠️  WARNING: {}\n", message);
}

/// Display an info message
pub fn info(message: &str) {
    println!("\nℹ️  {}\n", message);
}

/// Display a success message
pub fn success(message: &str) {
    println!("\n✅ {}\n", message);
}

/// Display an error message
#[allow(dead_code)] // Public API for error display
pub fn error(message: &str) {
    eprintln!("\n❌ ERROR: {}\n", message);
}

/// Display a step in a multi-step process
pub fn step(number: usize, total: usize, message: &str) {
    println!("\n[{}/{}] {}", number, total, message);
}

/// Prompt for a timestamp with validation
pub fn input_timestamp(prompt: &str, default: Option<&str>) -> Result<String> {
    loop {
        let value = input(prompt, default)?;

        // Basic validation - check if it looks like a timestamp
        if value.contains('T') || value.contains('-') {
            return Ok(value);
        }

        eprintln!("Invalid timestamp format. Expected RFC3339 format like: 2025-01-15T10:30:00Z");
    }
}

/// Prompt for a path with optional validation
pub fn input_path(prompt: &str, default: Option<&str>, must_exist: bool) -> Result<String> {
    loop {
        let value = input(prompt, default)?;

        if !must_exist {
            return Ok(value);
        }

        let path = std::path::Path::new(&value);
        if path.exists() {
            return Ok(value);
        }

        eprintln!("Path does not exist: {}", value);
    }
}

/// Interactive PITR restore configuration
#[allow(dead_code)] // Public API for PITR wizard
pub struct PitrRestoreConfig {
    pub target_time: String,
    pub target_dir: String,
    pub backup_dir: String,
    pub auto_start: bool,
    pub confirmed: bool,
}

/// Run interactive PITR restore wizard
pub fn pitr_restore_wizard() -> Result<PitrRestoreConfig> {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Point-in-Time Recovery (PITR) Wizard               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    info("This wizard will guide you through restoring your PostgreSQL database to a specific point in time.");

    // Step 1: Backup directory
    step(1, 5, "Specify backup location");
    let backup_dir = input_path(
        "Backup directory",
        Some("./backups"),
        true,
    )?;

    // Step 2: Target time
    step(2, 5, "Specify target recovery time");
    println!("  Enter the timestamp you want to recover to.");
    println!("  Format: RFC3339 (e.g., 2025-01-15T10:30:00Z)");
    let target_time = input_timestamp("Target time", None)?;

    // Step 3: Target directory
    step(3, 5, "Specify target directory for recovered database");
    let target_dir = input(
        "Target directory",
        Some("/var/lib/postgresql/data-recovered"),
    )?;

    // Step 4: Auto-start option
    step(4, 5, "Post-recovery options");
    let auto_start = confirm("Start PostgreSQL automatically after recovery?", false)?;

    // Step 5: Summary and confirmation
    step(5, 5, "Review and confirm");

    display_summary(
        "PITR Recovery Plan",
        &[
            ("Backup Directory", &backup_dir),
            ("Target Time", &target_time),
            ("Target Directory", &target_dir),
            ("Auto-start", if auto_start { "Yes" } else { "No" }),
        ],
    );

    warn("This operation will create a new PostgreSQL data directory. Ensure you have sufficient disk space.");

    let confirmed = confirm("Proceed with recovery?", false)?;

    Ok(PitrRestoreConfig {
        target_time,
        target_dir,
        backup_dir,
        auto_start,
        confirmed,
    })
}

/// Interactive HA switchover configuration
#[allow(dead_code)] // Public API for HA wizard
pub struct HaSwitchoverConfig {
    pub cluster: String,
    pub from_node: String,
    pub to_node: String,
    pub max_lag_bytes: u64,
    pub confirmed: bool,
}

/// Run interactive HA switchover wizard
#[allow(dead_code)] // Public API for HA wizard
pub fn ha_switchover_wizard(
    clusters: &[&str],
    nodes: &[(&str, &str, &str)], // (id, cluster, role)
) -> Result<HaSwitchoverConfig> {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║              HA Switchover Wizard                            ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    info("This wizard will guide you through a planned switchover from primary to replica.");

    // Step 1: Select cluster
    step(1, 5, "Select cluster");
    let cluster_idx = if clusters.len() == 1 {
        println!("  Using cluster: {}", clusters[0]);
        0
    } else {
        select("Select cluster", clusters, 0)?
    };
    let cluster = clusters[cluster_idx].to_string();

    // Filter nodes by cluster
    let cluster_nodes: Vec<_> = nodes
        .iter()
        .filter(|(_, c, _)| *c == cluster)
        .collect();

    // Step 2: Select source (primary) node
    step(2, 5, "Select source node (current primary)");
    let primary_nodes: Vec<_> = cluster_nodes
        .iter()
        .filter(|(_, _, role)| *role == "primary")
        .map(|(id, _, _)| *id)
        .collect();

    let from_node = if primary_nodes.len() == 1 {
        println!("  Using primary: {}", primary_nodes[0]);
        primary_nodes[0].to_string()
    } else if primary_nodes.is_empty() {
        return Err(anyhow!("No primary node found in cluster {}", cluster));
    } else {
        let idx = select("Select source node", &primary_nodes, 0)?;
        primary_nodes[idx].to_string()
    };

    // Step 3: Select target (replica) node
    step(3, 5, "Select target node (replica to promote)");
    let replica_nodes: Vec<_> = cluster_nodes
        .iter()
        .filter(|(_, _, role)| *role == "replica")
        .map(|(id, _, _)| *id)
        .collect();

    let to_node = if replica_nodes.is_empty() {
        return Err(anyhow!("No replica nodes found in cluster {}", cluster));
    } else if replica_nodes.len() == 1 {
        println!("  Using replica: {}", replica_nodes[0]);
        replica_nodes[0].to_string()
    } else {
        let idx = select("Select target node", &replica_nodes, 0)?;
        replica_nodes[idx].to_string()
    };

    // Step 4: Configure options
    step(4, 5, "Configure switchover options");
    let max_lag_str = input("Maximum replication lag (bytes)", Some("1048576"))?;
    let max_lag_bytes: u64 = max_lag_str
        .parse()
        .map_err(|_| anyhow!("Invalid number: {}", max_lag_str))?;

    // Step 5: Summary and confirmation
    step(5, 5, "Review and confirm");

    display_summary(
        "Switchover Plan",
        &[
            ("Cluster", &cluster),
            ("From Node (Primary)", &from_node),
            ("To Node (Replica)", &to_node),
            ("Max Replication Lag", &format!("{} bytes", max_lag_bytes)),
        ],
    );

    warn("This operation will change the primary node of your cluster. Ensure all applications can handle a brief interruption.");

    let confirmed = confirm("Proceed with switchover?", false)?;

    Ok(HaSwitchoverConfig {
        cluster,
        from_node,
        to_node,
        max_lag_bytes,
        confirmed,
    })
}

/// Interactive cluster validation with fix suggestions
#[allow(dead_code)] // Public API for cluster validation wizard
pub struct ClusterValidateConfig {
    pub config_path: Option<String>,
    pub auto_fix: bool,
}

/// Run interactive cluster validation wizard
pub fn cluster_validate_wizard() -> Result<ClusterValidateConfig> {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Cluster Configuration Validator                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    info("This wizard will validate your cluster configuration and suggest fixes for any issues.");

    // Step 1: Config path
    step(1, 2, "Specify configuration file");
    let use_default = confirm("Use default configuration paths?", true)?;

    let config_path = if use_default {
        None
    } else {
        Some(input_path("Configuration file path", Some("./cluster.yaml"), true)?)
    };

    // Step 2: Auto-fix option
    step(2, 2, "Validation options");
    let auto_fix = confirm("Attempt to auto-fix simple issues?", false)?;

    Ok(ClusterValidateConfig {
        config_path,
        auto_fix,
    })
}

/// Print a progress bar
#[allow(dead_code)] // Public API for progress display
pub fn progress_bar(current: usize, total: usize, width: usize) {
    let progress = (current as f64 / total as f64 * width as f64) as usize;
    let remaining = width - progress;

    print!("\r[");
    for _ in 0..progress {
        print!("█");
    }
    for _ in 0..remaining {
        print!("░");
    }
    print!("] {}/{}", current, total);
    io::stdout().flush().unwrap();

    if current == total {
        println!();
    }
}
