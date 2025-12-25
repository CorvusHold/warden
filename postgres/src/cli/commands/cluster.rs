//! Cluster configuration CLI commands.
//!
//! These commands provide offline-first cluster introspection, working entirely
//! with local configuration files without requiring HOLD or C2.

use anyhow::{anyhow, Result};
use common::config::{ClusterConfig, ClusterConfigError, NodeRole};
use log::info;
use serde::Serialize;
use std::convert::Infallible;
use std::path::Path;
use std::str::FromStr;

/// Output format for cluster commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
}

impl FromStr for OutputFormat {
    type Err = Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "json" => OutputFormat::Json,
            _ => OutputFormat::Table,
        })
    }
}

/// Result of cluster validation.
#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationErrorInfo>,
    pub summary: ValidationSummary,
}

#[derive(Debug, Serialize)]
pub struct ValidationErrorInfo {
    pub path: String,
    pub message: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct ValidationSummary {
    pub clusters: usize,
    pub nodes: usize,
    pub protection_groups: usize,
}

/// Validate a cluster configuration file.
pub fn cluster_validate(config_path: Option<&Path>) -> Result<ValidationResult> {
    info!("[cluster-validate] Loading cluster configuration...");

    // Try to load the config
    let config = match ClusterConfig::load(config_path) {
        Ok(c) => c,
        Err(ClusterConfigError::NotFound(paths)) => {
            return Ok(ValidationResult {
                valid: false,
                errors: vec![ValidationErrorInfo {
                    path: "".to_string(),
                    message: format!(
                        "Cluster configuration file not found. Searched: {}",
                        paths.join(", ")
                    ),
                    code: "FILE_NOT_FOUND".to_string(),
                }],
                summary: ValidationSummary {
                    clusters: 0,
                    nodes: 0,
                    protection_groups: 0,
                },
            });
        }
        Err(ClusterConfigError::Io(e)) => {
            return Ok(ValidationResult {
                valid: false,
                errors: vec![ValidationErrorInfo {
                    path: "".to_string(),
                    message: format!("Failed to read configuration file: {}", e),
                    code: "IO_ERROR".to_string(),
                }],
                summary: ValidationSummary {
                    clusters: 0,
                    nodes: 0,
                    protection_groups: 0,
                },
            });
        }
        Err(ClusterConfigError::Parse(e)) => {
            return Ok(ValidationResult {
                valid: false,
                errors: vec![ValidationErrorInfo {
                    path: "".to_string(),
                    message: format!("Failed to parse YAML: {}", e),
                    code: "PARSE_ERROR".to_string(),
                }],
                summary: ValidationSummary {
                    clusters: 0,
                    nodes: 0,
                    protection_groups: 0,
                },
            });
        }
        Err(e) => return Err(anyhow!("Failed to load cluster config: {}", e)),
    };

    // Validate the config
    let validation_errors = config.validate();

    let errors: Vec<ValidationErrorInfo> = validation_errors
        .iter()
        .map(|e| ValidationErrorInfo {
            path: e.path.clone(),
            message: e.message.clone(),
            code: e.code.to_string(),
        })
        .collect();

    let result = ValidationResult {
        valid: errors.is_empty(),
        errors,
        summary: ValidationSummary {
            clusters: config.clusters.len(),
            nodes: config.nodes.len(),
            protection_groups: config.protection_groups.len(),
        },
    };

    Ok(result)
}

/// Format validation result for display.
pub fn format_validation_result(result: &ValidationResult, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(result).unwrap_or_default(),
        OutputFormat::Table => {
            let mut output = String::new();

            if result.valid {
                output.push_str("✓ Cluster configuration is valid\n\n");
            } else {
                output.push_str("✗ Cluster configuration has errors\n\n");
                output.push_str("Errors:\n");
                for error in &result.errors {
                    if error.path.is_empty() {
                        output.push_str(&format!("  - [{}] {}\n", error.code, error.message));
                    } else {
                        output.push_str(&format!(
                            "  - {} [{}]: {}\n",
                            error.path, error.code, error.message
                        ));
                    }
                }
                output.push('\n');
            }

            output.push_str("Summary:\n");
            output.push_str(&format!(
                "  Clusters:          {}\n",
                result.summary.clusters
            ));
            output.push_str(&format!("  Nodes:             {}\n", result.summary.nodes));
            output.push_str(&format!(
                "  Protection Groups: {}\n",
                result.summary.protection_groups
            ));

            output
        }
    }
}

/// Cluster overview information.
#[derive(Debug, Serialize)]
pub struct ClusterOverview {
    pub clusters: Vec<ClusterInfo>,
}

#[derive(Debug, Serialize)]
pub struct ClusterInfo {
    pub id: String,
    pub name: Option<String>,
    pub environment: Option<String>,
    pub node_count: usize,
    pub primary_count: usize,
    pub replica_count: usize,
    pub protection_group_count: usize,
    pub labels: std::collections::HashMap<String, String>,
}

/// Show cluster configuration overview.
pub fn cluster_show(config_path: Option<&Path>) -> Result<ClusterOverview> {
    info!("[cluster-show] Loading cluster configuration...");

    let config = ClusterConfig::load_and_validate(config_path).map_err(|e| anyhow!("{}", e))?;

    let clusters: Vec<ClusterInfo> = config
        .clusters
        .iter()
        .map(|c| {
            let nodes = config.get_nodes_by_cluster(&c.id);
            let primaries = config.get_nodes_by_cluster_and_role(&c.id, NodeRole::Primary);
            let replicas = config.get_nodes_by_cluster_and_role(&c.id, NodeRole::Replica);
            let pgs = config.get_protection_groups_by_cluster(&c.id);

            ClusterInfo {
                id: c.id.clone(),
                name: c.name.clone(),
                environment: c.environment.clone(),
                node_count: nodes.len(),
                primary_count: primaries.len(),
                replica_count: replicas.len(),
                protection_group_count: pgs.len(),
                labels: c.labels.clone(),
            }
        })
        .collect();

    Ok(ClusterOverview { clusters })
}

/// Format cluster overview for display.
pub fn format_cluster_overview(overview: &ClusterOverview, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(overview).unwrap_or_default(),
        OutputFormat::Table => {
            let mut output = String::new();

            if overview.clusters.is_empty() {
                output.push_str("No clusters defined in configuration.\n");
                return output;
            }

            output.push_str("Clusters:\n");
            output.push_str(&format!(
                "{:<20} {:<25} {:<15} {:<8} {:<10} {:<8} {:<6}\n",
                "ID", "NAME", "ENVIRONMENT", "NODES", "PRIMARIES", "REPLICAS", "PGs"
            ));
            output.push_str(&"-".repeat(100));
            output.push('\n');

            for cluster in &overview.clusters {
                output.push_str(&format!(
                    "{:<20} {:<25} {:<15} {:<8} {:<10} {:<8} {:<6}\n",
                    cluster.id,
                    cluster.name.as_deref().unwrap_or("-"),
                    cluster.environment.as_deref().unwrap_or("-"),
                    cluster.node_count,
                    cluster.primary_count,
                    cluster.replica_count,
                    cluster.protection_group_count,
                ));
            }

            output
        }
    }
}

/// Node information for display.
#[derive(Debug, Serialize)]
pub struct NodeList {
    pub nodes: Vec<NodeInfo>,
}

#[derive(Debug, Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub cluster_id: String,
    pub host: String,
    pub port: u16,
    pub role: String,
    pub labels: std::collections::HashMap<String, String>,
    pub has_ssh: bool,
    pub has_connection_config: bool,
}

/// List nodes in the cluster configuration.
pub fn cluster_nodes(
    config_path: Option<&Path>,
    cluster_filter: Option<&str>,
    role_filter: Option<&str>,
) -> Result<NodeList> {
    info!("[cluster-nodes] Loading cluster configuration...");

    let config = ClusterConfig::load_and_validate(config_path).map_err(|e| anyhow!("{}", e))?;

    // Parse role filter
    let role_filter: Option<NodeRole> = role_filter.and_then(|r| match r.to_lowercase().as_str() {
        "primary" => Some(NodeRole::Primary),
        "replica" => Some(NodeRole::Replica),
        "unknown" => Some(NodeRole::Unknown),
        _ => None,
    });

    let nodes: Vec<NodeInfo> = config
        .nodes
        .iter()
        .filter(|n| {
            // Apply cluster filter
            if let Some(cluster_id) = cluster_filter {
                if n.cluster_id != cluster_id {
                    return false;
                }
            }
            // Apply role filter
            if let Some(role) = role_filter {
                if n.role != role {
                    return false;
                }
            }
            true
        })
        .map(|n| NodeInfo {
            id: n.id.clone(),
            cluster_id: n.cluster_id.clone(),
            host: n.host.clone(),
            port: n.port,
            role: n.role.to_string(),
            labels: n.labels.clone(),
            has_ssh: n.ssh.is_some(),
            has_connection_config: n.connection.is_some(),
        })
        .collect();

    Ok(NodeList { nodes })
}

/// Format node list for display.
pub fn format_node_list(list: &NodeList, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(list).unwrap_or_default(),
        OutputFormat::Table => {
            let mut output = String::new();

            if list.nodes.is_empty() {
                output.push_str("No nodes found matching the filter criteria.\n");
                return output;
            }

            output.push_str("Nodes:\n");
            output.push_str(&format!(
                "{:<20} {:<20} {:<30} {:<8} {:<10} {:<5} {:<5}\n",
                "ID", "CLUSTER", "HOST", "PORT", "ROLE", "SSH", "CONN"
            ));
            output.push_str(&"-".repeat(100));
            output.push('\n');

            for node in &list.nodes {
                output.push_str(&format!(
                    "{:<20} {:<20} {:<30} {:<8} {:<10} {:<5} {:<5}\n",
                    node.id,
                    node.cluster_id,
                    node.host,
                    node.port,
                    node.role,
                    if node.has_ssh { "✓" } else { "-" },
                    if node.has_connection_config {
                        "✓"
                    } else {
                        "-"
                    },
                ));
            }

            output.push_str(&format!("\nTotal: {} node(s)\n", list.nodes.len()));

            output
        }
    }
}

/// Protection group information for display.
#[derive(Debug, Serialize)]
pub struct ProtectionGroupList {
    pub protection_groups: Vec<ProtectionGroupInfo>,
}

#[derive(Debug, Serialize)]
pub struct ProtectionGroupInfo {
    pub id: String,
    pub name: Option<String>,
    pub cluster_id: String,
    pub databases: Vec<String>,
    pub preferred_source_role: Option<String>,
    pub labels: std::collections::HashMap<String, String>,
}

/// List protection groups in the cluster configuration.
pub fn cluster_protection_groups(
    config_path: Option<&Path>,
    cluster_filter: Option<&str>,
) -> Result<ProtectionGroupList> {
    info!("[cluster-protection-groups] Loading cluster configuration...");

    let config = ClusterConfig::load_and_validate(config_path).map_err(|e| anyhow!("{}", e))?;

    let protection_groups: Vec<ProtectionGroupInfo> = config
        .protection_groups
        .iter()
        .filter(|pg| {
            // Apply cluster filter
            if let Some(cluster_id) = cluster_filter {
                if pg.cluster_id != cluster_id {
                    return false;
                }
            }
            true
        })
        .map(|pg| ProtectionGroupInfo {
            id: pg.id.clone(),
            name: pg.name.clone(),
            cluster_id: pg.cluster_id.clone(),
            databases: pg.databases.clone(),
            preferred_source_role: pg.preferred_source_role.map(|r| r.to_string()),
            labels: pg.labels.clone(),
        })
        .collect();

    Ok(ProtectionGroupList { protection_groups })
}

/// Format protection group list for display.
pub fn format_protection_group_list(list: &ProtectionGroupList, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(list).unwrap_or_default(),
        OutputFormat::Table => {
            let mut output = String::new();

            if list.protection_groups.is_empty() {
                output.push_str("No protection groups found matching the filter criteria.\n");
                return output;
            }

            output.push_str("Protection Groups:\n");
            output.push_str(&format!(
                "{:<20} {:<25} {:<20} {:<15} {:<30}\n",
                "ID", "NAME", "CLUSTER", "PREF. ROLE", "DATABASES"
            ));
            output.push_str(&"-".repeat(115));
            output.push('\n');

            for pg in &list.protection_groups {
                let databases_str = if pg.databases.len() > 3 {
                    format!(
                        "{}, ... (+{})",
                        pg.databases[..3].join(", "),
                        pg.databases.len() - 3
                    )
                } else {
                    pg.databases.join(", ")
                };

                output.push_str(&format!(
                    "{:<20} {:<25} {:<20} {:<15} {:<30}\n",
                    pg.id,
                    pg.name.as_deref().unwrap_or("-"),
                    pg.cluster_id,
                    pg.preferred_source_role.as_deref().unwrap_or("any"),
                    databases_str,
                ));
            }

            output.push_str(&format!(
                "\nTotal: {} protection group(s)\n",
                list.protection_groups.len()
            ));

            output
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_config() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        let config = r#"
version: "1"
clusters:
  - id: "test-cluster"
    name: "Test Cluster"
    environment: "test"
nodes:
  - id: "node-1"
    cluster_id: "test-cluster"
    host: "localhost"
    port: 5432
    role: primary
  - id: "node-2"
    cluster_id: "test-cluster"
    host: "localhost"
    port: 5433
    role: replica
protection_groups:
  - id: "pg-1"
    name: "Test PG"
    cluster_id: "test-cluster"
    databases:
      - "testdb"
    preferred_source_role: replica
"#;
        file.write_all(config.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_cluster_validate_valid_config() {
        let file = create_test_config();
        let result = cluster_validate(Some(file.path())).unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
        assert_eq!(result.summary.clusters, 1);
        assert_eq!(result.summary.nodes, 2);
        assert_eq!(result.summary.protection_groups, 1);
    }

    #[test]
    fn test_cluster_validate_missing_file() {
        let result = cluster_validate(Some(Path::new("/nonexistent/path.yaml"))).unwrap();
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_cluster_show() {
        let file = create_test_config();
        let overview = cluster_show(Some(file.path())).unwrap();
        assert_eq!(overview.clusters.len(), 1);
        assert_eq!(overview.clusters[0].id, "test-cluster");
        assert_eq!(overview.clusters[0].node_count, 2);
        assert_eq!(overview.clusters[0].primary_count, 1);
        assert_eq!(overview.clusters[0].replica_count, 1);
    }

    #[test]
    fn test_cluster_nodes() {
        let file = create_test_config();
        let list = cluster_nodes(Some(file.path()), None, None).unwrap();
        assert_eq!(list.nodes.len(), 2);
    }

    #[test]
    fn test_cluster_nodes_with_role_filter() {
        let file = create_test_config();
        let list = cluster_nodes(Some(file.path()), None, Some("primary")).unwrap();
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(list.nodes[0].role, "primary");
    }

    #[test]
    fn test_cluster_protection_groups() {
        let file = create_test_config();
        let list = cluster_protection_groups(Some(file.path()), None).unwrap();
        assert_eq!(list.protection_groups.len(), 1);
        assert_eq!(list.protection_groups[0].id, "pg-1");
    }

    #[test]
    fn test_format_validation_result_table() {
        let result = ValidationResult {
            valid: true,
            errors: vec![],
            summary: ValidationSummary {
                clusters: 1,
                nodes: 2,
                protection_groups: 1,
            },
        };
        let output = format_validation_result(&result, OutputFormat::Table);
        assert!(output.contains("valid"));
        assert!(output.contains("Clusters:"));
    }

    #[test]
    fn test_format_validation_result_json() {
        let result = ValidationResult {
            valid: true,
            errors: vec![],
            summary: ValidationSummary {
                clusters: 1,
                nodes: 2,
                protection_groups: 1,
            },
        };
        let output = format_validation_result(&result, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["valid"], true);
    }
}
