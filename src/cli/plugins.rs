//! CLI commands for plugin management.
//!
//! This module provides commands for listing and inspecting available
//! data source plugins.

use common::datasource::{BackupType, DataSourceCapabilities, PluginInfo, PluginRegistry};
use std::sync::Arc;

/// List all available plugins.
///
/// Displays a table of registered plugins with their name, version,
/// PITR support, incremental support, and description.
pub fn list_plugins(registry: &PluginRegistry) {
    let plugins = registry.list();

    if plugins.is_empty() {
        println!("No plugins registered.");
        return;
    }

    // Print header
    println!(
        "{:<15} {:<10} {:<6} {:<12} DESCRIPTION",
        "NAME", "VERSION", "PITR", "INCREMENTAL"
    );
    println!("{}", "-".repeat(70));

    // Print each plugin
    for plugin in plugins {
        let pitr = if plugin.capabilities.supports_pitr {
            "✓"
        } else {
            "✗"
        };
        let incremental = if plugin.capabilities.supports_incremental {
            "✓"
        } else {
            "✗"
        };

        println!(
            "{:<15} {:<10} {:<6} {:<12} {}",
            plugin.name, plugin.version, pitr, incremental, plugin.description
        );
    }
}

/// Show detailed information about a specific plugin.
///
/// Displays comprehensive information including capabilities,
/// supported backup types, and custom features.
pub fn show_plugin_info(registry: &PluginRegistry, name: &str) {
    match registry.info(name) {
        Some(info) => print_plugin_details(&info),
        None => {
            eprintln!("Plugin '{}' not found.", name);
            eprintln!();
            eprintln!("Available plugins:");
            for plugin in registry.list() {
                eprintln!("  - {}", plugin.name);
            }
            std::process::exit(1);
        }
    }
}

/// Print detailed plugin information.
fn print_plugin_details(info: &PluginInfo) {
    println!("Name:        {}", info.name);
    println!("Version:     {}", info.version);
    println!("Description: {}", info.description);
    println!();

    println!("Capabilities:");
    print_capabilities(&info.capabilities);
    println!();

    println!("Backup Types:");
    for backup_type in &info.capabilities.backup_types {
        let description = match backup_type {
            BackupType::Full => "Complete backup of all data",
            BackupType::Incremental => "Changes since last backup",
            BackupType::Snapshot => "Point-in-time logical backup",
            BackupType::Differential => "Changes since last full backup",
        };
        println!("  - {}: {}", backup_type, description);
    }

    if !info.capabilities.custom.is_empty() {
        println!();
        println!("Custom Features:");
        for (feature, enabled) in &info.capabilities.custom {
            let status = if *enabled { "✓" } else { "✗" };
            let feature_name = feature.replace('_', " ");
            println!("  {} {}", status, feature_name);
        }
    }
}

/// Print capabilities in a formatted list.
fn print_capabilities(caps: &DataSourceCapabilities) {
    let items = [
        ("Point-in-Time Recovery (PITR)", caps.supports_pitr),
        ("Incremental backups", caps.supports_incremental),
        ("Logical backups", caps.supports_logical_backup),
        ("Physical backups", caps.supports_physical_backup),
        ("SSH tunnel support", caps.supports_ssh_tunnel),
        ("Remote storage (S3/MinIO)", caps.supports_remote_storage),
        ("High Availability", caps.supports_ha),
        ("Encryption", caps.supports_encryption),
        ("Compression", caps.supports_compression),
    ];

    for (name, enabled) in items {
        let status = if enabled { "✓" } else { "✗" };
        println!("  {} {}", status, name);
    }
}

/// Initialize the plugin registry with default plugins.
///
/// This function registers all compile-time enabled plugins.
/// Currently, only PostgreSQL is supported.
pub fn init_registry() -> PluginRegistry {
    let mut registry = PluginRegistry::new();

    // Register PostgreSQL plugin (always available)
    let pg_plugin = Arc::new(postgres::PostgresDataSource::new());
    if let Err(e) = registry.register(pg_plugin) {
        log::warn!("Failed to register PostgreSQL plugin: {}", e);
    }

    // Future plugins would be registered here based on feature flags:
    // #[cfg(feature = "mysql")]
    // registry.register(Arc::new(mysql::MySqlDataSource::new()))?;
    //
    // #[cfg(feature = "mongodb")]
    // registry.register(Arc::new(mongodb::MongoDataSource::new()))?;

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_registry() {
        let registry = init_registry();
        assert!(registry.contains("postgresql"));
    }

    #[test]
    fn test_list_plugins() {
        let registry = init_registry();
        // Just verify it doesn't panic
        list_plugins(&registry);
    }
}
