//! Cluster configuration model for PostgreSQL HA topologies.
//!
//! This module provides types and utilities for describing PostgreSQL cluster
//! topologies including clusters, nodes, roles, and protection groups.
//!
//! The configuration is fully offline and does not require any external services.
//!
//! # Example
//!
//! ```yaml
//! version: "1"
//!
//! clusters:
//!   - id: "prod-billing"
//!     name: "Production Billing Cluster"
//!     environment: "production"
//!
//! nodes:
//!   - id: "billing-primary"
//!     cluster_id: "prod-billing"
//!     host: "db-primary.internal"
//!     port: 5432
//!     role: "primary"
//!
//! protection_groups:
//!   - id: "billing-dbs"
//!     cluster_id: "prod-billing"
//!     databases:
//!       - "billing_main"
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

/// Top-level cluster configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Schema version for forward compatibility.
    #[serde(default = "default_version")]
    pub version: String,

    /// Default tenant for all clusters in this config.
    /// Can be overridden per-cluster.
    #[serde(default)]
    pub default_tenant: Option<String>,

    /// List of cluster definitions.
    #[serde(default)]
    pub clusters: Vec<Cluster>,

    /// List of node definitions.
    #[serde(default)]
    pub nodes: Vec<Node>,

    /// List of protection group definitions.
    #[serde(default)]
    pub protection_groups: Vec<ProtectionGroup>,
}

fn default_version() -> String {
    "1".to_string()
}

/// A PostgreSQL cluster definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    /// Unique identifier for the cluster.
    pub id: String,

    /// Human-readable name for the cluster.
    #[serde(default)]
    pub name: Option<String>,

    /// Tenant identifier (organization/project) for this cluster.
    /// Overrides the default_tenant from ClusterConfig if set.
    #[serde(default)]
    pub tenant: Option<String>,

    /// Environment tag (e.g., "production", "staging", "development").
    #[serde(default)]
    pub environment: Option<String>,

    /// Arbitrary key-value labels for filtering and organization.
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// A PostgreSQL node (instance) within a cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier for the node.
    pub id: String,

    /// Reference to the parent cluster ID.
    pub cluster_id: String,

    /// Hostname or IP address of the PostgreSQL instance.
    pub host: String,

    /// PostgreSQL port (default: 5432).
    #[serde(default = "default_port")]
    pub port: u16,

    /// Role of this node in the cluster.
    pub role: NodeRole,

    /// Arbitrary key-value labels for filtering and organization.
    #[serde(default)]
    pub labels: HashMap<String, String>,

    /// Default connection parameters for this node.
    #[serde(default)]
    pub connection: Option<ConnectionConfig>,

    /// SSH tunnel configuration for remote access.
    #[serde(default)]
    pub ssh: Option<SshConfig>,
}

fn default_port() -> u16 {
    5432
}

/// Role of a node within a PostgreSQL cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeRole {
    /// Primary (read-write) node.
    Primary,
    /// Replica (read-only) node.
    Replica,
    /// Role is unknown or not yet determined.
    Unknown,
}

impl fmt::Display for NodeRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeRole::Primary => write!(f, "primary"),
            NodeRole::Replica => write!(f, "replica"),
            NodeRole::Unknown => write!(f, "unknown"),
        }
    }
}

impl NodeRole {
    /// Returns all valid role values as strings.
    pub fn valid_values() -> &'static [&'static str] {
        &["primary", "replica", "unknown"]
    }
}

/// PostgreSQL connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// PostgreSQL user for authentication.
    #[serde(default)]
    pub user: Option<String>,

    /// Database name to connect to.
    #[serde(default)]
    pub database: Option<String>,

    /// SSL mode (disable, allow, prefer, require, verify-ca, verify-full).
    #[serde(default)]
    pub ssl_mode: Option<String>,

    /// Environment variable name containing the password.
    /// Passwords should never be stored directly in config.
    #[serde(default)]
    pub password_env: Option<String>,
}

/// SSH tunnel configuration for remote database access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    /// SSH bastion/jump host.
    pub host: String,

    /// SSH username.
    #[serde(default)]
    pub user: Option<String>,

    /// SSH port (default: 22).
    #[serde(default = "default_ssh_port")]
    pub port: u16,

    /// Path to SSH private key file.
    #[serde(default)]
    pub key_path: Option<String>,

    /// Environment variable name containing SSH password.
    #[serde(default)]
    pub password_env: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}

/// A protection group defines a set of databases that are protected together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionGroup {
    /// Unique identifier for the protection group.
    pub id: String,

    /// Human-readable name for the protection group.
    #[serde(default)]
    pub name: Option<String>,

    /// Reference to the parent cluster ID.
    pub cluster_id: String,

    /// List of database names to protect together.
    pub databases: Vec<String>,

    /// Preferred node role for backup operations.
    /// If set, backups will prefer nodes with this role.
    #[serde(default)]
    pub preferred_source_role: Option<NodeRole>,

    /// Arbitrary key-value labels for filtering and organization.
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

/// Validation error for cluster configuration.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Path to the problematic field (e.g., "nodes[0].cluster_id").
    pub path: String,
    /// Human-readable error message.
    pub message: String,
    /// Error code for programmatic handling.
    pub code: ValidationErrorCode,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.path, self.message, self.code)
    }
}

impl std::error::Error for ValidationError {}

/// Error codes for validation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationErrorCode {
    /// A required field is missing.
    MissingRequired,
    /// A duplicate ID was found.
    DuplicateId,
    /// A reference to another entity is invalid.
    InvalidReference,
    /// A field value is invalid.
    InvalidValue,
    /// The schema version is unsupported.
    UnsupportedVersion,
}

impl fmt::Display for ValidationErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationErrorCode::MissingRequired => write!(f, "MISSING_REQUIRED"),
            ValidationErrorCode::DuplicateId => write!(f, "DUPLICATE_ID"),
            ValidationErrorCode::InvalidReference => write!(f, "INVALID_REFERENCE"),
            ValidationErrorCode::InvalidValue => write!(f, "INVALID_VALUE"),
            ValidationErrorCode::UnsupportedVersion => write!(f, "UNSUPPORTED_VERSION"),
        }
    }
}

/// Error type for cluster configuration loading.
#[derive(Debug)]
pub enum ClusterConfigError {
    /// File not found at any of the searched paths.
    NotFound(Vec<String>),
    /// IO error reading the file.
    Io(std::io::Error),
    /// YAML parsing error.
    Parse(serde_yaml::Error),
    /// Validation errors in the configuration.
    Validation(Vec<ValidationError>),
}

impl fmt::Display for ClusterConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClusterConfigError::NotFound(paths) => {
                write!(
                    f,
                    "Cluster config not found. Searched: {}",
                    paths.join(", ")
                )
            }
            ClusterConfigError::Io(e) => write!(f, "IO error reading cluster config: {}", e),
            ClusterConfigError::Parse(e) => write!(f, "Failed to parse cluster config: {}", e),
            ClusterConfigError::Validation(errors) => {
                writeln!(
                    f,
                    "Cluster config validation failed with {} error(s):",
                    errors.len()
                )?;
                for err in errors {
                    writeln!(f, "  - {}", err)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ClusterConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClusterConfigError::Io(e) => Some(e),
            ClusterConfigError::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ClusterConfigError {
    fn from(err: std::io::Error) -> Self {
        ClusterConfigError::Io(err)
    }
}

impl From<serde_yaml::Error> for ClusterConfigError {
    fn from(err: serde_yaml::Error) -> Self {
        ClusterConfigError::Parse(err)
    }
}

impl ClusterConfig {
    /// Default paths to search for cluster configuration.
    pub fn default_paths() -> Vec<String> {
        vec![
            "cluster.yaml".to_string(),
            "~/.warden/cluster.yaml".to_string(),
            "/etc/warden/cluster.yaml".to_string(),
        ]
    }

    /// Load cluster configuration from a file path.
    ///
    /// If `path` is `None`, searches default paths in order.
    pub fn load(path: Option<&Path>) -> Result<Self, ClusterConfigError> {
        let content = if let Some(p) = path {
            std::fs::read_to_string(p)?
        } else {
            let paths = Self::default_paths();
            let mut found = None;

            for p in &paths {
                let expanded = shellexpand::full(p)
                    .unwrap_or_else(|_| p.into())
                    .into_owned();
                if Path::new(&expanded).exists() {
                    found = Some(std::fs::read_to_string(&expanded)?);
                    break;
                }
            }

            found.ok_or(ClusterConfigError::NotFound(paths))?
        };

        let config: ClusterConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Load and validate cluster configuration.
    ///
    /// Returns the config if valid, or a validation error with all issues found.
    pub fn load_and_validate(path: Option<&Path>) -> Result<Self, ClusterConfigError> {
        let config = Self::load(path)?;
        let errors = config.validate();
        if errors.is_empty() {
            Ok(config)
        } else {
            Err(ClusterConfigError::Validation(errors))
        }
    }

    /// Validate the cluster configuration.
    ///
    /// Returns a list of all validation errors found. An empty list means the config is valid.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        // Check version
        if self.version != "1" {
            errors.push(ValidationError {
                path: "version".to_string(),
                message: format!(
                    "Unsupported schema version '{}'. Supported versions: 1",
                    self.version
                ),
                code: ValidationErrorCode::UnsupportedVersion,
            });
        }

        // Collect cluster IDs and check for duplicates
        let mut cluster_ids: HashSet<&str> = HashSet::new();
        for (i, cluster) in self.clusters.iter().enumerate() {
            if cluster.id.is_empty() {
                errors.push(ValidationError {
                    path: format!("clusters[{}].id", i),
                    message: "Cluster ID cannot be empty".to_string(),
                    code: ValidationErrorCode::MissingRequired,
                });
            } else if !cluster_ids.insert(&cluster.id) {
                errors.push(ValidationError {
                    path: format!("clusters[{}].id", i),
                    message: format!("Duplicate cluster ID '{}'", cluster.id),
                    code: ValidationErrorCode::DuplicateId,
                });
            }
        }

        // Collect node IDs and validate nodes
        let mut node_ids: HashSet<&str> = HashSet::new();
        for (i, node) in self.nodes.iter().enumerate() {
            // Check for empty ID
            if node.id.is_empty() {
                errors.push(ValidationError {
                    path: format!("nodes[{}].id", i),
                    message: "Node ID cannot be empty".to_string(),
                    code: ValidationErrorCode::MissingRequired,
                });
            } else if !node_ids.insert(&node.id) {
                errors.push(ValidationError {
                    path: format!("nodes[{}].id", i),
                    message: format!("Duplicate node ID '{}'", node.id),
                    code: ValidationErrorCode::DuplicateId,
                });
            }

            // Check cluster reference
            if node.cluster_id.is_empty() {
                errors.push(ValidationError {
                    path: format!("nodes[{}].cluster_id", i),
                    message: "Node cluster_id cannot be empty".to_string(),
                    code: ValidationErrorCode::MissingRequired,
                });
            } else if !cluster_ids.contains(node.cluster_id.as_str()) {
                errors.push(ValidationError {
                    path: format!("nodes[{}].cluster_id", i),
                    message: format!(
                        "Node '{}' references non-existent cluster '{}'",
                        node.id, node.cluster_id
                    ),
                    code: ValidationErrorCode::InvalidReference,
                });
            }

            // Check host
            if node.host.is_empty() {
                errors.push(ValidationError {
                    path: format!("nodes[{}].host", i),
                    message: "Node host cannot be empty".to_string(),
                    code: ValidationErrorCode::MissingRequired,
                });
            }

            // Check port range
            if node.port == 0 {
                errors.push(ValidationError {
                    path: format!("nodes[{}].port", i),
                    message: "Node port cannot be 0".to_string(),
                    code: ValidationErrorCode::InvalidValue,
                });
            }
        }

        // Collect protection group IDs and validate
        let mut pg_ids: HashSet<&str> = HashSet::new();
        for (i, pg) in self.protection_groups.iter().enumerate() {
            // Check for empty ID
            if pg.id.is_empty() {
                errors.push(ValidationError {
                    path: format!("protection_groups[{}].id", i),
                    message: "Protection group ID cannot be empty".to_string(),
                    code: ValidationErrorCode::MissingRequired,
                });
            } else if !pg_ids.insert(&pg.id) {
                errors.push(ValidationError {
                    path: format!("protection_groups[{}].id", i),
                    message: format!("Duplicate protection group ID '{}'", pg.id),
                    code: ValidationErrorCode::DuplicateId,
                });
            }

            // Check cluster reference
            if pg.cluster_id.is_empty() {
                errors.push(ValidationError {
                    path: format!("protection_groups[{}].cluster_id", i),
                    message: "Protection group cluster_id cannot be empty".to_string(),
                    code: ValidationErrorCode::MissingRequired,
                });
            } else if !cluster_ids.contains(pg.cluster_id.as_str()) {
                errors.push(ValidationError {
                    path: format!("protection_groups[{}].cluster_id", i),
                    message: format!(
                        "Protection group '{}' references non-existent cluster '{}'",
                        pg.id, pg.cluster_id
                    ),
                    code: ValidationErrorCode::InvalidReference,
                });
            }

            // Check databases list
            if pg.databases.is_empty() {
                errors.push(ValidationError {
                    path: format!("protection_groups[{}].databases", i),
                    message: "Protection group must have at least one database".to_string(),
                    code: ValidationErrorCode::MissingRequired,
                });
            }

            // Check for empty database names
            for (j, db) in pg.databases.iter().enumerate() {
                if db.is_empty() {
                    errors.push(ValidationError {
                        path: format!("protection_groups[{}].databases[{}]", i, j),
                        message: "Database name cannot be empty".to_string(),
                        code: ValidationErrorCode::InvalidValue,
                    });
                }
            }
        }

        errors
    }

    /// Get all clusters.
    pub fn get_clusters(&self) -> &[Cluster] {
        &self.clusters
    }

    /// Get a cluster by ID.
    pub fn get_cluster(&self, id: &str) -> Option<&Cluster> {
        self.clusters.iter().find(|c| c.id == id)
    }

    /// Get all nodes.
    pub fn get_nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Get all nodes belonging to a specific cluster.
    pub fn get_nodes_by_cluster(&self, cluster_id: &str) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|n| n.cluster_id == cluster_id)
            .collect()
    }

    /// Get all nodes with a specific role.
    pub fn get_nodes_by_role(&self, role: NodeRole) -> Vec<&Node> {
        self.nodes.iter().filter(|n| n.role == role).collect()
    }

    /// Get all nodes in a cluster with a specific role.
    pub fn get_nodes_by_cluster_and_role(&self, cluster_id: &str, role: NodeRole) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|n| n.cluster_id == cluster_id && n.role == role)
            .collect()
    }

    /// Get all protection groups.
    pub fn get_protection_groups(&self) -> &[ProtectionGroup] {
        &self.protection_groups
    }

    /// Get a protection group by ID.
    pub fn get_protection_group(&self, id: &str) -> Option<&ProtectionGroup> {
        self.protection_groups.iter().find(|pg| pg.id == id)
    }

    /// Get all protection groups belonging to a specific cluster.
    pub fn get_protection_groups_by_cluster(&self, cluster_id: &str) -> Vec<&ProtectionGroup> {
        self.protection_groups
            .iter()
            .filter(|pg| pg.cluster_id == cluster_id)
            .collect()
    }

    /// Get the primary node for a cluster, if one exists.
    pub fn get_primary_node(&self, cluster_id: &str) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|n| n.cluster_id == cluster_id && n.role == NodeRole::Primary)
    }

    /// Get all replica nodes for a cluster.
    pub fn get_replica_nodes(&self, cluster_id: &str) -> Vec<&Node> {
        self.get_nodes_by_cluster_and_role(cluster_id, NodeRole::Replica)
    }

    /// Get the preferred backup source node for a protection group.
    ///
    /// Returns a node based on the protection group's `preferred_source_role`:
    /// - If set to `Primary`, returns the primary node.
    /// - If set to `Replica`, returns the first replica node.
    /// - If not set or `Unknown`, returns the primary node (fallback).
    pub fn get_preferred_backup_source(&self, protection_group_id: &str) -> Option<&Node> {
        let pg = self.get_protection_group(protection_group_id)?;

        match pg.preferred_source_role {
            Some(NodeRole::Replica) => {
                // Prefer replica, fall back to primary
                self.get_replica_nodes(&pg.cluster_id)
                    .first()
                    .copied()
                    .or_else(|| self.get_primary_node(&pg.cluster_id))
            }
            Some(NodeRole::Primary) | None | Some(NodeRole::Unknown) => {
                // Prefer primary, fall back to first replica
                self.get_primary_node(&pg.cluster_id)
                    .or_else(|| self.get_replica_nodes(&pg.cluster_id).first().copied())
            }
        }
    }

    /// Get the effective tenant for a cluster.
    ///
    /// Returns the cluster's tenant if set, otherwise falls back to the default_tenant.
    pub fn get_effective_tenant(&self, cluster_id: &str) -> Option<&str> {
        self.get_cluster(cluster_id)
            .and_then(|c| c.tenant.as_deref())
            .or(self.default_tenant.as_deref())
    }

    /// Get all clusters belonging to a specific tenant.
    pub fn get_clusters_by_tenant(&self, tenant: &str) -> Vec<&Cluster> {
        self.clusters
            .iter()
            .filter(|c| {
                c.tenant.as_deref() == Some(tenant)
                    || (c.tenant.is_none() && self.default_tenant.as_deref() == Some(tenant))
            })
            .collect()
    }

    /// Get all unique tenants in this configuration.
    pub fn get_tenants(&self) -> Vec<&str> {
        let mut tenants: HashSet<&str> = HashSet::new();

        // Add default tenant if set
        if let Some(ref t) = self.default_tenant {
            tenants.insert(t.as_str());
        }

        // Add cluster-specific tenants
        for cluster in &self.clusters {
            if let Some(ref t) = cluster.tenant {
                tenants.insert(t.as_str());
            }
        }

        tenants.into_iter().collect()
    }
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            version: default_version(),
            default_tenant: None,
            clusters: Vec::new(),
            nodes: Vec::new(),
            protection_groups: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> ClusterConfig {
        ClusterConfig {
            version: "1".to_string(),
            default_tenant: None,
            clusters: vec![Cluster {
                id: "test-cluster".to_string(),
                name: Some("Test Cluster".to_string()),
                tenant: None,
                environment: Some("test".to_string()),
                labels: HashMap::new(),
            }],
            nodes: vec![
                Node {
                    id: "node-1".to_string(),
                    cluster_id: "test-cluster".to_string(),
                    host: "localhost".to_string(),
                    port: 5432,
                    role: NodeRole::Primary,
                    labels: HashMap::new(),
                    connection: None,
                    ssh: None,
                },
                Node {
                    id: "node-2".to_string(),
                    cluster_id: "test-cluster".to_string(),
                    host: "localhost".to_string(),
                    port: 5433,
                    role: NodeRole::Replica,
                    labels: HashMap::new(),
                    connection: None,
                    ssh: None,
                },
            ],
            protection_groups: vec![ProtectionGroup {
                id: "pg-1".to_string(),
                name: Some("Test PG".to_string()),
                cluster_id: "test-cluster".to_string(),
                databases: vec!["testdb".to_string()],
                preferred_source_role: Some(NodeRole::Replica),
                labels: HashMap::new(),
            }],
        }
    }

    #[test]
    fn test_valid_config() {
        let config = sample_config();
        let errors = config.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_duplicate_cluster_id() {
        let mut config = sample_config();
        config.clusters.push(Cluster {
            id: "test-cluster".to_string(), // Duplicate
            name: None,
            tenant: None,
            environment: None,
            labels: HashMap::new(),
        });

        let errors = config.validate();
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::DuplicateId));
    }

    #[test]
    fn test_duplicate_node_id() {
        let mut config = sample_config();
        config.nodes.push(Node {
            id: "node-1".to_string(), // Duplicate
            cluster_id: "test-cluster".to_string(),
            host: "localhost".to_string(),
            port: 5434,
            role: NodeRole::Replica,
            labels: HashMap::new(),
            connection: None,
            ssh: None,
        });

        let errors = config.validate();
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::DuplicateId));
    }

    #[test]
    fn test_invalid_cluster_reference() {
        let mut config = sample_config();
        config.nodes.push(Node {
            id: "orphan-node".to_string(),
            cluster_id: "non-existent".to_string(), // Invalid reference
            host: "localhost".to_string(),
            port: 5435,
            role: NodeRole::Replica,
            labels: HashMap::new(),
            connection: None,
            ssh: None,
        });

        let errors = config.validate();
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::InvalidReference));
    }

    #[test]
    fn test_empty_databases() {
        let mut config = sample_config();
        config.protection_groups.push(ProtectionGroup {
            id: "empty-pg".to_string(),
            name: None,
            cluster_id: "test-cluster".to_string(),
            databases: vec![], // Empty
            preferred_source_role: None,
            labels: HashMap::new(),
        });

        let errors = config.validate();
        assert!(errors
            .iter()
            .any(|e| e.code == ValidationErrorCode::MissingRequired));
    }

    #[test]
    fn test_get_nodes_by_cluster() {
        let config = sample_config();
        let nodes = config.get_nodes_by_cluster("test-cluster");
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_get_primary_node() {
        let config = sample_config();
        let primary = config.get_primary_node("test-cluster");
        assert!(primary.is_some());
        assert_eq!(primary.unwrap().id, "node-1");
    }

    #[test]
    fn test_get_replica_nodes() {
        let config = sample_config();
        let replicas = config.get_replica_nodes("test-cluster");
        assert_eq!(replicas.len(), 1);
        assert_eq!(replicas[0].id, "node-2");
    }

    #[test]
    fn test_get_preferred_backup_source_replica() {
        let config = sample_config();
        let source = config.get_preferred_backup_source("pg-1");
        assert!(source.is_some());
        // Should prefer replica since preferred_source_role is Replica
        assert_eq!(source.unwrap().id, "node-2");
    }

    #[test]
    fn test_yaml_deserialization() {
        let yaml = r#"
version: "1"
clusters:
  - id: "my-cluster"
    name: "My Cluster"
    environment: "production"
nodes:
  - id: "primary"
    cluster_id: "my-cluster"
    host: "db.example.com"
    port: 5432
    role: primary
protection_groups:
  - id: "main-dbs"
    cluster_id: "my-cluster"
    databases:
      - "app_db"
      - "audit_db"
"#;

        let config: ClusterConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.clusters.len(), 1);
        assert_eq!(config.nodes.len(), 1);
        assert_eq!(config.protection_groups.len(), 1);
        assert_eq!(config.protection_groups[0].databases.len(), 2);

        let errors = config.validate();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_node_role_display() {
        assert_eq!(NodeRole::Primary.to_string(), "primary");
        assert_eq!(NodeRole::Replica.to_string(), "replica");
        assert_eq!(NodeRole::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_tenant_from_cluster() {
        let mut config = sample_config();
        config.clusters[0].tenant = Some("acme-corp".to_string());

        let tenant = config.get_effective_tenant("test-cluster");
        assert_eq!(tenant, Some("acme-corp"));
    }

    #[test]
    fn test_tenant_from_default() {
        let mut config = sample_config();
        config.default_tenant = Some("default-tenant".to_string());

        let tenant = config.get_effective_tenant("test-cluster");
        assert_eq!(tenant, Some("default-tenant"));
    }

    #[test]
    fn test_cluster_tenant_overrides_default() {
        let mut config = sample_config();
        config.default_tenant = Some("default-tenant".to_string());
        config.clusters[0].tenant = Some("cluster-tenant".to_string());

        let tenant = config.get_effective_tenant("test-cluster");
        assert_eq!(tenant, Some("cluster-tenant"));
    }

    #[test]
    fn test_get_clusters_by_tenant() {
        let mut config = sample_config();
        config.default_tenant = Some("default-tenant".to_string());
        config.clusters.push(Cluster {
            id: "other-cluster".to_string(),
            name: None,
            tenant: Some("other-tenant".to_string()),
            environment: None,
            labels: HashMap::new(),
        });

        let default_clusters = config.get_clusters_by_tenant("default-tenant");
        assert_eq!(default_clusters.len(), 1);
        assert_eq!(default_clusters[0].id, "test-cluster");

        let other_clusters = config.get_clusters_by_tenant("other-tenant");
        assert_eq!(other_clusters.len(), 1);
        assert_eq!(other_clusters[0].id, "other-cluster");
    }

    #[test]
    fn test_get_tenants() {
        let mut config = sample_config();
        config.default_tenant = Some("default-tenant".to_string());
        config.clusters.push(Cluster {
            id: "other-cluster".to_string(),
            name: None,
            tenant: Some("other-tenant".to_string()),
            environment: None,
            labels: HashMap::new(),
        });

        let tenants = config.get_tenants();
        assert_eq!(tenants.len(), 2);
        assert!(tenants.contains(&"default-tenant"));
        assert!(tenants.contains(&"other-tenant"));
    }

    #[test]
    fn test_yaml_with_tenant() {
        let yaml = r#"
version: "1"
default_tenant: "acme-corp"
clusters:
  - id: "prod-cluster"
    name: "Production"
    environment: "production"
  - id: "staging-cluster"
    name: "Staging"
    tenant: "staging-tenant"
    environment: "staging"
nodes:
  - id: "primary"
    cluster_id: "prod-cluster"
    host: "db.example.com"
    port: 5432
    role: primary
protection_groups:
  - id: "main-dbs"
    cluster_id: "prod-cluster"
    databases:
      - "app_db"
"#;

        let config: ClusterConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.default_tenant, Some("acme-corp".to_string()));
        assert_eq!(config.clusters[0].tenant, None);
        assert_eq!(
            config.clusters[1].tenant,
            Some("staging-tenant".to_string())
        );

        // Test effective tenant resolution
        assert_eq!(
            config.get_effective_tenant("prod-cluster"),
            Some("acme-corp")
        );
        assert_eq!(
            config.get_effective_tenant("staging-cluster"),
            Some("staging-tenant")
        );
    }
}
