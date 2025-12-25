//! Storage key path builder for multi-tenant and multi-cluster layouts.
//!
//! This module provides utilities for constructing S3/MinIO object keys
//! that support hierarchical organization by tenant, cluster, protection group,
//! and database.
//!
//! # Key Layout Patterns
//!
//! ## Full Multi-Tenant Layout
//! ```text
//! <tenant>/<cluster_id>/<protection_group>/<database>/<backup_id>/...
//! ```
//!
//! ## Cluster-Only Layout
//! ```text
//! <prefix>/<cluster_id>/<protection_group>/<database>/<backup_id>/...
//! ```
//!
//! ## Legacy Layout
//! ```text
//! <prefix>/<backup_id>/...
//! ```

use serde::{Deserialize, Serialize};

/// Context for building storage keys.
///
/// This struct holds the hierarchical context (tenant, cluster, protection group,
/// database) used to construct storage keys. All fields are optional to support
/// various layout patterns from full multi-tenant to legacy flat layouts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageKeyContext {
    /// Tenant identifier (organization/project).
    pub tenant: Option<String>,
    /// Cluster identifier.
    pub cluster_id: Option<String>,
    /// Protection group identifier.
    pub protection_group: Option<String>,
    /// Database name.
    pub database: Option<String>,
    /// Legacy prefix (used when tenant is not set).
    pub prefix: Option<String>,
}

impl StorageKeyContext {
    /// Creates a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tenant.
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    /// Sets the cluster ID.
    pub fn with_cluster(mut self, cluster_id: impl Into<String>) -> Self {
        self.cluster_id = Some(cluster_id.into());
        self
    }

    /// Sets the protection group.
    pub fn with_protection_group(mut self, protection_group: impl Into<String>) -> Self {
        self.protection_group = Some(protection_group.into());
        self
    }

    /// Sets the database name.
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Sets the legacy prefix.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Returns true if this context uses the multi-tenant layout.
    pub fn is_multi_tenant(&self) -> bool {
        self.tenant.is_some()
    }

    /// Returns true if this context uses the legacy flat layout.
    pub fn is_legacy(&self) -> bool {
        self.tenant.is_none() && self.cluster_id.is_none()
    }
}

/// Builder for constructing storage keys based on context.
#[derive(Debug, Clone)]
pub struct StorageKeyBuilder {
    context: StorageKeyContext,
}

impl StorageKeyBuilder {
    /// Creates a new key builder with the given context.
    pub fn new(context: StorageKeyContext) -> Self {
        Self { context }
    }

    /// Creates a key builder from individual components.
    #[allow(clippy::too_many_arguments)]
    pub fn from_components(
        tenant: Option<String>,
        cluster_id: Option<String>,
        protection_group: Option<String>,
        database: Option<String>,
        prefix: Option<String>,
    ) -> Self {
        Self {
            context: StorageKeyContext {
                tenant,
                cluster_id,
                protection_group,
                database,
                prefix,
            },
        }
    }

    /// Returns the base prefix for listing/searching backups.
    ///
    /// This returns the common prefix for all backups in the current context,
    /// useful for listing operations.
    pub fn list_prefix(&self) -> String {
        self.build_base_path()
    }

    /// Builds the key for a backup directory.
    pub fn backup_key(&self, backup_id: &str) -> String {
        let base = self.build_base_path();
        if base.is_empty() {
            backup_id.to_string()
        } else {
            format!("{}/{}", base, backup_id)
        }
    }

    /// Builds the key for a backup metadata file.
    pub fn metadata_key(&self, backup_id: &str) -> String {
        format!("{}/backup_metadata.json", self.backup_key(backup_id))
    }

    /// Builds the key for a specific file within a backup.
    pub fn backup_file_key(&self, backup_id: &str, file_name: &str) -> String {
        format!("{}/{}", self.backup_key(backup_id), file_name)
    }

    /// Builds the key for WAL segments.
    ///
    /// WAL segments are stored at the protection group or database level,
    /// not per-backup.
    pub fn wal_key(&self, segment: &str) -> String {
        let base = self.build_wal_base_path();
        if base.is_empty() {
            format!("wal/{}", segment)
        } else {
            format!("{}/wal/{}", base, segment)
        }
    }

    /// Builds the prefix for listing WAL segments.
    pub fn wal_prefix(&self) -> String {
        let base = self.build_wal_base_path();
        if base.is_empty() {
            "wal/".to_string()
        } else {
            format!("{}/wal/", base)
        }
    }

    /// Builds the key for the retention policy file.
    ///
    /// Retention policies are stored at the cluster or tenant level.
    pub fn retention_policy_key(&self) -> String {
        let base = self.build_retention_base_path();
        if base.is_empty() {
            "retention_policy.json".to_string()
        } else {
            format!("{}/retention_policy.json", base)
        }
    }

    /// Builds the key for tenant metadata.
    pub fn tenant_metadata_key(&self) -> Option<String> {
        self.context
            .tenant
            .as_ref()
            .map(|t| format!("{}/tenant_metadata.json", t))
    }

    /// Returns the context used by this builder.
    pub fn context(&self) -> &StorageKeyContext {
        &self.context
    }

    /// Builds the base path for backups based on context.
    fn build_base_path(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();

        // Multi-tenant layout: tenant/cluster/pg/db
        if let Some(ref tenant) = self.context.tenant {
            parts.push(tenant);
            if let Some(ref cluster) = self.context.cluster_id {
                parts.push(cluster);
                if let Some(ref pg) = self.context.protection_group {
                    parts.push(pg);
                    if let Some(ref db) = self.context.database {
                        parts.push(db);
                    }
                } else if let Some(ref db) = self.context.database {
                    // tenant/cluster/db (no protection group)
                    parts.push(db);
                }
            } else if let Some(ref db) = self.context.database {
                // tenant/db (no cluster)
                parts.push(db);
            }
        } else {
            // Legacy/cluster-only layout: prefix/cluster/pg/db or prefix/backup_id
            if let Some(ref prefix) = self.context.prefix {
                if !prefix.is_empty() {
                    parts.push(prefix);
                }
            }
            if let Some(ref cluster) = self.context.cluster_id {
                parts.push(cluster);
                if let Some(ref pg) = self.context.protection_group {
                    parts.push(pg);
                    if let Some(ref db) = self.context.database {
                        parts.push(db);
                    }
                } else if let Some(ref db) = self.context.database {
                    parts.push(db);
                }
            } else if let Some(ref db) = self.context.database {
                // prefix/db (database-only mode)
                parts.push(db);
            }
        }

        parts.join("/")
    }

    /// Builds the base path for WAL segments.
    ///
    /// WAL is stored at the protection group level if available,
    /// otherwise at the cluster or tenant level.
    fn build_wal_base_path(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();

        if let Some(ref tenant) = self.context.tenant {
            parts.push(tenant);
            if let Some(ref cluster) = self.context.cluster_id {
                parts.push(cluster);
                if let Some(ref pg) = self.context.protection_group {
                    parts.push(pg);
                }
            }
        } else {
            if let Some(ref prefix) = self.context.prefix {
                if !prefix.is_empty() {
                    parts.push(prefix);
                }
            }
            if let Some(ref cluster) = self.context.cluster_id {
                parts.push(cluster);
                if let Some(ref pg) = self.context.protection_group {
                    parts.push(pg);
                }
            }
        }

        parts.join("/")
    }

    /// Builds the base path for retention policy.
    ///
    /// Retention policy is stored at the cluster level if available,
    /// otherwise at the tenant or prefix level.
    fn build_retention_base_path(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();

        if let Some(ref tenant) = self.context.tenant {
            parts.push(tenant);
            if let Some(ref cluster) = self.context.cluster_id {
                parts.push(cluster);
            }
        } else {
            if let Some(ref prefix) = self.context.prefix {
                if !prefix.is_empty() {
                    parts.push(prefix);
                }
            }
            if let Some(ref cluster) = self.context.cluster_id {
                parts.push(cluster);
            }
        }

        parts.join("/")
    }
}

impl Default for StorageKeyBuilder {
    fn default() -> Self {
        Self::new(StorageKeyContext::default())
    }
}

impl From<StorageKeyContext> for StorageKeyBuilder {
    fn from(context: StorageKeyContext) -> Self {
        Self::new(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_layout() {
        let builder = StorageKeyBuilder::new(StorageKeyContext::new());

        assert_eq!(builder.backup_key("backup-123"), "backup-123");
        assert_eq!(
            builder.metadata_key("backup-123"),
            "backup-123/backup_metadata.json"
        );
        assert_eq!(
            builder.wal_key("000000010000000000000001"),
            "wal/000000010000000000000001"
        );
        assert_eq!(builder.retention_policy_key(), "retention_policy.json");
    }

    #[test]
    fn test_legacy_with_prefix() {
        let builder = StorageKeyBuilder::new(StorageKeyContext::new().with_prefix("postgres"));

        assert_eq!(builder.backup_key("backup-123"), "postgres/backup-123");
        assert_eq!(
            builder.metadata_key("backup-123"),
            "postgres/backup-123/backup_metadata.json"
        );
        assert_eq!(
            builder.wal_key("000000010000000000000001"),
            "postgres/wal/000000010000000000000001"
        );
        assert_eq!(
            builder.retention_policy_key(),
            "postgres/retention_policy.json"
        );
    }

    #[test]
    fn test_database_only_layout() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_prefix("backups")
                .with_database("mydb"),
        );

        assert_eq!(builder.backup_key("backup-123"), "backups/mydb/backup-123");
        assert_eq!(
            builder.metadata_key("backup-123"),
            "backups/mydb/backup-123/backup_metadata.json"
        );
    }

    #[test]
    fn test_cluster_only_layout() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_prefix("backups")
                .with_cluster("prod-billing")
                .with_database("billing_main"),
        );

        assert_eq!(
            builder.backup_key("backup-123"),
            "backups/prod-billing/billing_main/backup-123"
        );
        assert_eq!(
            builder.metadata_key("backup-123"),
            "backups/prod-billing/billing_main/backup-123/backup_metadata.json"
        );
        assert_eq!(
            builder.wal_key("000000010000000000000001"),
            "backups/prod-billing/wal/000000010000000000000001"
        );
        assert_eq!(
            builder.retention_policy_key(),
            "backups/prod-billing/retention_policy.json"
        );
    }

    #[test]
    fn test_cluster_with_protection_group() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_prefix("backups")
                .with_cluster("prod-billing")
                .with_protection_group("billing-core")
                .with_database("billing_main"),
        );

        assert_eq!(
            builder.backup_key("backup-123"),
            "backups/prod-billing/billing-core/billing_main/backup-123"
        );
        assert_eq!(
            builder.wal_key("000000010000000000000001"),
            "backups/prod-billing/billing-core/wal/000000010000000000000001"
        );
    }

    #[test]
    fn test_multi_tenant_layout() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_tenant("acme-corp")
                .with_cluster("prod-billing")
                .with_protection_group("billing-core")
                .with_database("billing_main"),
        );

        assert_eq!(
            builder.backup_key("backup-123"),
            "acme-corp/prod-billing/billing-core/billing_main/backup-123"
        );
        assert_eq!(
            builder.metadata_key("backup-123"),
            "acme-corp/prod-billing/billing-core/billing_main/backup-123/backup_metadata.json"
        );
        assert_eq!(
            builder.wal_key("000000010000000000000001"),
            "acme-corp/prod-billing/billing-core/wal/000000010000000000000001"
        );
        assert_eq!(
            builder.retention_policy_key(),
            "acme-corp/prod-billing/retention_policy.json"
        );
        assert_eq!(
            builder.tenant_metadata_key(),
            Some("acme-corp/tenant_metadata.json".to_string())
        );
    }

    #[test]
    fn test_tenant_without_cluster() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_tenant("acme-corp")
                .with_database("mydb"),
        );

        assert_eq!(
            builder.backup_key("backup-123"),
            "acme-corp/mydb/backup-123"
        );
        assert_eq!(builder.wal_key("seg1"), "acme-corp/wal/seg1");
        assert_eq!(
            builder.retention_policy_key(),
            "acme-corp/retention_policy.json"
        );
    }

    #[test]
    fn test_tenant_with_cluster_no_pg() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_tenant("acme-corp")
                .with_cluster("prod-billing")
                .with_database("billing_main"),
        );

        assert_eq!(
            builder.backup_key("backup-123"),
            "acme-corp/prod-billing/billing_main/backup-123"
        );
        assert_eq!(builder.wal_key("seg1"), "acme-corp/prod-billing/wal/seg1");
    }

    #[test]
    fn test_list_prefix() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_tenant("acme-corp")
                .with_cluster("prod-billing"),
        );

        assert_eq!(builder.list_prefix(), "acme-corp/prod-billing");
    }

    #[test]
    fn test_backup_file_key() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_tenant("acme-corp")
                .with_cluster("prod-billing")
                .with_database("mydb"),
        );

        assert_eq!(
            builder.backup_file_key("backup-123", "pg_dump.dump"),
            "acme-corp/prod-billing/mydb/backup-123/pg_dump.dump"
        );
    }

    #[test]
    fn test_is_multi_tenant() {
        let legacy = StorageKeyContext::new().with_prefix("backups");
        assert!(!legacy.is_multi_tenant());
        assert!(legacy.is_legacy());

        let cluster_only = StorageKeyContext::new()
            .with_prefix("backups")
            .with_cluster("prod");
        assert!(!cluster_only.is_multi_tenant());
        assert!(!cluster_only.is_legacy());

        let multi_tenant = StorageKeyContext::new()
            .with_tenant("acme")
            .with_cluster("prod");
        assert!(multi_tenant.is_multi_tenant());
        assert!(!multi_tenant.is_legacy());
    }

    #[test]
    fn test_wal_prefix() {
        let builder = StorageKeyBuilder::new(
            StorageKeyContext::new()
                .with_tenant("acme-corp")
                .with_cluster("prod-billing")
                .with_protection_group("billing-core"),
        );

        assert_eq!(
            builder.wal_prefix(),
            "acme-corp/prod-billing/billing-core/wal/"
        );
    }

    #[test]
    fn test_from_components() {
        let builder = StorageKeyBuilder::from_components(
            Some("tenant".to_string()),
            Some("cluster".to_string()),
            Some("pg".to_string()),
            Some("db".to_string()),
            None,
        );

        assert_eq!(
            builder.backup_key("backup-1"),
            "tenant/cluster/pg/db/backup-1"
        );
    }
}
