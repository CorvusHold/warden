//! Extended help topics for Warden CLI
//!
//! Provides detailed documentation for complex features via `warden help <topic>`.

use std::collections::HashMap;

/// Available help topics
#[allow(dead_code)] // Public API for help system
pub const TOPICS: &[&str] = &["backup", "pitr", "ha", "config", "storage", "retention"];

/// Get the help content for a given topic
pub fn get_topic_help(topic: &str) -> Option<&'static str> {
    let topics: HashMap<&str, &str> = HashMap::from([
        ("backup", BACKUP_HELP),
        ("pitr", PITR_HELP),
        ("ha", HA_HELP),
        ("config", CONFIG_HELP),
        ("storage", STORAGE_HELP),
        ("retention", RETENTION_HELP),
    ]);

    topics.get(topic).copied()
}

/// List all available topics with descriptions
pub fn list_topics() -> String {
    r#"
Available Documentation Topics
==============================

  backup     - Backup concepts, types, and workflows
  pitr       - Point-in-Time Recovery concepts and examples
  ha         - High Availability orchestration guide
  config     - Configuration file reference
  storage    - S3-compatible storage setup
  retention  - Retention policies and purge operations

Usage: warden docs <topic>

Example: warden docs backup
"#
    .to_string()
}

const BACKUP_HELP: &str = r#"
BACKUP CONCEPTS AND WORKFLOWS
=============================

Warden supports multiple backup types for PostgreSQL databases:

BACKUP TYPES
------------

1. Snapshot Backup (Recommended)
   Creates both physical and logical backups in a single operation.
   - Physical: pg_basebackup for full cluster backup
   - Logical: pg_dump for portable SQL dump
   
   Example:
     warden postgresql snapshot-backup \
       --database mydb \
       --user postgres \
       --backup-dir ./backups

2. Full Backup
   Creates a complete physical backup using pg_basebackup.
   Suitable for large databases where you need fast restore.
   
   Example:
     warden postgresql full-backup \
       --database mydb \
       --user postgres

3. Incremental Backup
   Captures changes since the last full backup using WAL archiving.
   Requires continuous WAL archiving to be configured.

BACKUP WORKFLOW
---------------

Basic local backup:
  warden postgresql snapshot-backup --database mydb --user postgres

Backup with S3 upload:
  warden postgresql snapshot-backup \
    --database mydb \
    --user postgres \
    --remote-storage \
    --storage-bucket my-backups \
    --storage-endpoint http://localhost:9000

Backup via SSH tunnel (remote database):
  warden postgresql snapshot-backup \
    --database mydb \
    --user postgres \
    --ssh-host bastion.example.com \
    --ssh-user ubuntu \
    --ssh-key-path ~/.ssh/id_rsa \
    --ssh-remote-port 5432

BACKUP VERIFICATION
-------------------

List available backups:
  warden postgresql backups list --storage-bucket my-backups

Inspect a specific backup:
  warden postgresql backups show --backup-id <ID> --storage-bucket my-backups

Verify backup integrity:
  warden postgresql backups download \
    --backup-id <ID> \
    --output ./verify \
    --verify-checksums \
    --storage-bucket my-backups

ENVIRONMENT VARIABLES
---------------------

PostgreSQL connection:
  PGHOST, PGPORT, PGDATABASE, PGUSER, PGPASSWORD, PGSSLMODE

S3/MinIO storage:
  AWS_BUCKET, AWS_REGION, AWS_ENDPOINT, AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY

BEST PRACTICES
--------------

1. Schedule regular backups (daily snapshot + continuous WAL archiving)
2. Store backups in remote storage (S3/MinIO) for disaster recovery
3. Test restores regularly to verify backup integrity
4. Use labels to organize backups: --label env=prod --label team=billing
5. Implement retention policies to manage storage costs

See also: warden help pitr, warden help retention, warden help storage
"#;

const PITR_HELP: &str = r#"
POINT-IN-TIME RECOVERY (PITR)
=============================

PITR allows you to restore a PostgreSQL database to any point in time
within your backup retention window. This is essential for recovering
from accidental data loss or corruption.

HOW PITR WORKS
--------------

1. Base Backup: A full backup serves as the starting point
2. WAL Segments: Write-Ahead Log files capture all changes
3. Recovery: Replay WAL from base backup to target time

PITR COMMANDS
-------------

1. List Recovery Options
   Shows available base backups and WAL coverage:
   
   warden postgresql pitr-list --backup-dir ./backups
   
   With remote storage:
   warden postgresql pitr-list \
     --remote-storage \
     --storage-bucket my-backups

2. Plan Recovery (Dry Run)
   Validates if recovery to a target time is possible:
   
   warden postgresql pitr-plan \
     --target-time "2025-01-15T10:30:00Z" \
     --backup-dir ./backups
   
   This shows:
   - Which base backup will be used
   - Required WAL segments
   - Any gaps or issues

3. Execute Recovery
   Performs the actual PITR:
   
   warden postgresql pitr-restore \
     --target-time "2025-01-15T10:30:00Z" \
     --backup-dir ./backups \
     --target-dir /var/lib/postgresql/data-recovered

PITR WORKFLOW EXAMPLE
---------------------

Step 1: Check available recovery points
  warden postgresql pitr-list --backup-dir ./backups

Step 2: Plan recovery to 10 minutes ago
  warden postgresql pitr-plan \
    --target-time "$(date -u -d '10 minutes ago' +%Y-%m-%dT%H:%M:%SZ)" \
    --backup-dir ./backups

Step 3: Execute recovery (after verification)
  warden postgresql pitr-restore \
    --target-time "2025-01-15T10:30:00Z" \
    --target-dir /var/lib/postgresql/data-recovered \
    --backup-dir ./backups \
    --auto-start

TARGET TIME FORMATS
-------------------

RFC3339 (recommended):
  2025-01-15T10:30:00Z
  2025-01-15T10:30:00+01:00

ISO 8601:
  2025-01-15T10:30:00

COMMON SCENARIOS
----------------

1. Recover from accidental DELETE:
   - Identify when the DELETE occurred
   - Set target-time to just before that moment
   - Restore to a new database for verification

2. Recover from application bug:
   - Find the deployment timestamp
   - Restore to just before deployment
   - Extract needed data from recovered instance

3. Audit/compliance point-in-time view:
   - Restore to the required audit timestamp
   - Run queries against recovered instance

REQUIREMENTS
------------

- Continuous WAL archiving must be enabled
- Base backups must be available
- WAL segments must cover the target time
- Sufficient disk space for recovery

TROUBLESHOOTING
---------------

"Target time is before earliest recovery point"
  → Your target time is before your oldest base backup

"WAL segment not found"
  → There's a gap in WAL archiving; recovery cannot proceed

"Target time is after latest recovery point"
  → WAL archiving may have stopped; check PostgreSQL logs

See also: warden help backup, warden help ha
"#;

const HA_HELP: &str = r#"
HIGH AVAILABILITY (HA) ORCHESTRATION
====================================

Warden provides commands for managing PostgreSQL HA clusters,
including planned switchovers and emergency failovers.

CLUSTER CONFIGURATION
---------------------

Define your cluster topology in cluster.yaml:

  version: "1"
  clusters:
    - id: "prod-billing"
      name: "Production Billing"
      environment: "production"
  
  nodes:
    - id: "billing-primary"
      cluster_id: "prod-billing"
      host: "db-primary.internal"
      port: 5432
      role: "primary"
    - id: "billing-replica-1"
      cluster_id: "prod-billing"
      host: "db-replica1.internal"
      port: 5432
      role: "replica"

Validate configuration:
  warden postgresql cluster-validate

View cluster overview:
  warden postgresql cluster-show

List nodes:
  warden postgresql cluster-nodes

HA OPERATIONS
-------------

1. PLANNED SWITCHOVER
   Gracefully transfer primary role to a replica.
   Use for maintenance, upgrades, or load balancing.
   
   Dry run (see the plan):
     warden postgresql ha-switchover \
       --cluster prod-billing \
       --from-node billing-primary \
       --to-node billing-replica-1 \
       --dry-run
   
   Execute switchover:
     warden postgresql ha-switchover \
       --cluster prod-billing \
       --from-node billing-primary \
       --to-node billing-replica-1 \
       --yes

   Switchover workflow:
   1. Verify both nodes are healthy
   2. Wait for replication to catch up
   3. Create checkpoint on primary
   4. Promote replica
   5. Verify new primary accepts writes
   6. Update cluster configuration

2. EMERGENCY FAILOVER
   Promote a replica when primary is unavailable.
   ⚠️  WARNING: May result in data loss!
   
   Dry run:
     warden postgresql ha-failover \
       --cluster prod-billing \
       --to-node billing-replica-1 \
       --dry-run
   
   Execute failover:
     warden postgresql ha-failover \
       --cluster prod-billing \
       --to-node billing-replica-1 \
       --yes
   
   Force failover (skip primary check):
     warden postgresql ha-failover \
       --cluster prod-billing \
       --to-node billing-replica-1 \
       --force \
       --yes

3. CLONE NODE
   Create a new replica from backup.
   
   From latest backup:
     warden postgresql ha-clone-node \
       --cluster prod-billing \
       --source-node billing-primary \
       --target-node billing-replica-2 \
       --target-dir /var/lib/postgresql/data
   
   From specific backup:
     warden postgresql ha-clone-node \
       --cluster prod-billing \
       --source-node billing-primary \
       --target-node billing-replica-2 \
       --backup-id abc123 \
       --target-dir /var/lib/postgresql/data

INTERACTIVE MODE
----------------

For guided operations with prompts and confirmations:

  warden postgresql ha-switchover --interactive
  warden postgresql ha-failover --interactive

Interactive mode will:
- Show current cluster state
- Ask for confirmation at each step
- Display progress and results

BEST PRACTICES
--------------

1. Always run --dry-run first to see the plan
2. Ensure backups are current before failover
3. Monitor replication lag before switchover
4. Test failover procedures regularly
5. Document your HA runbook

TROUBLESHOOTING
---------------

"Replication lag too high"
  → Wait for replica to catch up, or increase --max-lag-bytes

"Primary is still reachable"
  → For failover, use --force if you're sure primary should be replaced

"Cannot connect to node"
  → Check network connectivity and PostgreSQL configuration

See also: warden help backup, warden help pitr
"#;

const CONFIG_HELP: &str = r#"
CONFIGURATION REFERENCE
=======================

Warden uses YAML configuration files for persistent settings.

CONFIGURATION FILE LOCATIONS
----------------------------

Search order (first found is used):
1. ./warden.yaml (current directory)
2. ~/.warden/config.yaml (user home)
3. /etc/warden/config.yaml (system-wide)

MAIN CONFIGURATION SCHEMA
-------------------------

# warden.yaml
c2_server: "https://hold.corvus.io"

c2_auth:
  id: "device-uuid"
  secret: "device-secret"

features:
  overwatch: true
  postgres_backup: true

storage_defaults:
  provider: "s3"
  bucket: "my-backups"
  region: "us-east-1"
  endpoint: "https://s3.amazonaws.com"

postgres_defaults:
  user: "postgres"
  port: 5432

schedules:
  # See schedule configuration below

CLUSTER CONFIGURATION
---------------------

Separate file for HA cluster topology:

Search order:
1. ./cluster.yaml
2. ~/.warden/cluster.yaml
3. /etc/warden/cluster.yaml

Schema:
  version: "1"
  default_tenant: "acme-corp"
  
  clusters:
    - id: "prod-billing"
      name: "Production Billing"
      tenant: "acme-corp"
      environment: "production"
      labels:
        team: "billing"
  
  nodes:
    - id: "billing-primary"
      cluster_id: "prod-billing"
      host: "db-primary.internal"
      port: 5432
      role: "primary"
      connection:
        user: "warden_backup"
        database: "postgres"
      ssh:
        host: "bastion.internal"
        user: "warden"
        key_path: "/etc/warden/ssh/key"
  
  protection_groups:
    - id: "billing-dbs"
      cluster_id: "prod-billing"
      databases:
        - "billing_main"
        - "billing_audit"
      preferred_source_role: "replica"

SCHEDULE CONFIGURATION
----------------------

Automated backup and retention schedules:

schedules:
  default_backup_dir: "./backups"
  
  storage_profiles:
    - name: "production-s3"
      provider: "s3"
      bucket: "my-backups"
      prefix: "postgres/"
      region: "us-east-1"
      access_key: "env:AWS_ACCESS_KEY_ID"
      secret_key: "env:AWS_SECRET_ACCESS_KEY"
  
  backups:
    - id: "daily-backup"
      name: "Daily Snapshot Backup"
      cron: "0 2 * * *"
      target:
        host: "localhost"
        port: 5432
        database: "mydb"
        user: "postgres"
      backup_type: "snapshot"
      storage_profile: "production-s3"
      enabled: true
  
  retention:
    - id: "daily-retention"
      name: "Daily Retention Cleanup"
      cron: "0 4 * * *"
      storage_profile: "production-s3"
      policy_file: "./retention-policy.json"
      apply: true
      enabled: true

ENVIRONMENT VARIABLE REFERENCES
-------------------------------

Use "env:VAR_NAME" to reference environment variables:

  access_key: "env:AWS_ACCESS_KEY_ID"
  secret_key: "env:AWS_SECRET_ACCESS_KEY"

CLI CONFIGURATION COMMANDS
--------------------------

View current configuration:
  warden console config get
  warden console config get --format json

Set configuration values:
  warden console config set c2_server "https://hold.corvus.io"
  warden console config set features.Overwatch true

Validate cluster configuration:
  warden postgresql cluster-validate

Validate schedule configuration:
  warden postgresql schedule-validate

See also: warden help storage, warden help retention
"#;

const STORAGE_HELP: &str = r#"
S3-COMPATIBLE STORAGE SETUP
===========================

Warden supports S3-compatible storage for remote backup storage,
including AWS S3, MinIO, and other compatible services.

SUPPORTED PROVIDERS
-------------------

- AWS S3
- MinIO
- DigitalOcean Spaces
- Backblaze B2
- Wasabi
- Any S3-compatible service

CONFIGURATION
-------------

Via command-line flags:
  --remote-storage \
  --storage-bucket my-backups \
  --storage-region us-east-1 \
  --storage-endpoint https://s3.amazonaws.com \
  --storage-access-key AKIAIOSFODNN7EXAMPLE \
  --storage-secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY

Via environment variables:
  export AWS_BUCKET=my-backups
  export AWS_REGION=us-east-1
  export AWS_ENDPOINT=https://s3.amazonaws.com
  export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
  export AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY

MINIO SETUP
-----------

1. Start MinIO:
   docker run -p 9000:9000 -p 9001:9001 \
     -e MINIO_ROOT_USER=minioadmin \
     -e MINIO_ROOT_PASSWORD=minioadmin \
     minio/minio server /data --console-address ":9001"

2. Create a bucket:
   mc alias set local http://localhost:9000 minioadmin minioadmin
   mc mb local/my-backups

3. Use with Warden:
   warden postgresql snapshot-backup \
     --database mydb \
     --remote-storage \
     --storage-bucket my-backups \
     --storage-endpoint http://localhost:9000 \
     --storage-access-key minioadmin \
     --storage-secret-key minioadmin

AWS S3 SETUP
------------

1. Create an S3 bucket in AWS Console or CLI:
   aws s3 mb s3://my-backups --region us-east-1

2. Create IAM credentials with S3 access

3. Use with Warden:
   warden postgresql snapshot-backup \
     --database mydb \
     --remote-storage \
     --storage-bucket my-backups \
     --storage-region us-east-1

MULTI-TENANT STORAGE LAYOUT
---------------------------

For organizations with multiple clusters, use tenant/cluster organization:

  warden postgresql snapshot-backup \
    --database mydb \
    --tenant acme-corp \
    --cluster prod-billing \
    --protection-group billing-dbs \
    --remote-storage \
    --storage-bucket my-backups

This creates the following S3 structure:
  my-backups/
    acme-corp/
      prod-billing/
        billing-dbs/
          mydb/
            <backup-id>/
              backup_metadata.json
              pg_dump.dump

STORAGE OPERATIONS
------------------

List backups in storage:
  warden postgresql backups list --storage-bucket my-backups

Show backup details:
  warden postgresql backups show --backup-id <ID> --storage-bucket my-backups

Download backup:
  warden postgresql backups download \
    --backup-id <ID> \
    --output ./restored \
    --storage-bucket my-backups

Verify checksums:
  warden postgresql backups download \
    --backup-id <ID> \
    --output ./restored \
    --verify-checksums \
    --storage-bucket my-backups

SECURITY BEST PRACTICES
-----------------------

1. Use IAM roles instead of access keys when possible
2. Enable bucket versioning for additional protection
3. Use server-side encryption (SSE-S3 or SSE-KMS)
4. Restrict bucket access with IAM policies
5. Enable access logging for audit trails
6. Use environment variables for credentials (not command line)

See also: warden help backup, warden help retention
"#;

const RETENTION_HELP: &str = r#"
RETENTION POLICIES AND PURGE
============================

Retention policies define how long backups are kept and when
they should be automatically deleted.

RETENTION POLICY SCHEMA
-----------------------

{
  "version": "1",
  "rules": [
    {
      "name": "Keep daily backups for 7 days",
      "retention_days": 7,
      "backup_types": ["snapshot", "full"],
      "min_count": 3
    },
    {
      "name": "Keep weekly backups for 4 weeks",
      "retention_days": 28,
      "backup_types": ["snapshot"],
      "labels": {"weekly": "true"},
      "min_count": 4
    },
    {
      "name": "Keep monthly backups for 1 year",
      "retention_days": 365,
      "backup_types": ["snapshot"],
      "labels": {"monthly": "true"},
      "min_count": 12
    }
  ],
  "wal_retention_days": 7,
  "min_recovery_window_hours": 24
}

POLICY PRESETS
--------------

Generate a policy from a preset:

Standard (7 days, 3 minimum):
  warden postgresql retention-init --output ./policy.json --preset standard

Conservative (30 days, 7 minimum):
  warden postgresql retention-init --output ./policy.json --preset conservative

Aggressive (3 days, 1 minimum):
  warden postgresql retention-init --output ./policy.json --preset aggressive

GFS (Grandfather-Father-Son):
  warden postgresql retention-init --output ./policy.json --preset gfs

RETENTION WORKFLOW
------------------

1. Create a retention policy:
   warden postgresql retention-init --output ./policy.json

2. Review and customize the policy

3. Preview what would be deleted (dry run):
   warden postgresql retention-plan \
     --policy-file ./policy.json \
     --backup-dir ./backups

4. Apply the policy (with confirmation):
   warden postgresql retention-apply \
     --policy-file ./policy.json \
     --backup-dir ./backups \
     --apply

5. Apply without confirmation (for automation):
   warden postgresql retention-apply \
     --policy-file ./policy.json \
     --backup-dir ./backups \
     --apply \
     --yes

REMOTE STORAGE RETENTION
------------------------

Apply retention to S3 backups:

  warden postgresql retention-plan \
    --policy-file ./policy.json \
    --remote-storage \
    --storage-bucket my-backups

  warden postgresql retention-apply \
    --policy-file ./policy.json \
    --remote-storage \
    --storage-bucket my-backups \
    --apply

COMBINED LOCAL AND REMOTE
-------------------------

Apply to both local and remote:

  warden postgresql retention-apply \
    --policy-file ./policy.json \
    --backup-dir ./backups \
    --include-local \
    --include-remote \
    --remote-storage \
    --storage-bucket my-backups \
    --apply

SCHEDULING RETENTION
--------------------

Add to your warden.yaml for automated retention:

schedules:
  retention:
    - id: "daily-retention"
      name: "Daily Retention Cleanup"
      cron: "0 4 * * *"
      storage_profile: "production-s3"
      policy_file: "./retention-policy.json"
      apply: true
      enabled: true

LABEL-BASED EXCEPTIONS
----------------------

Protect specific backups from deletion using labels:

Create backup with protection label:
  warden postgresql snapshot-backup \
    --database mydb \
    --label protected=true

Policy rule to keep protected backups longer:
  {
    "name": "Keep protected backups for 1 year",
    "retention_days": 365,
    "labels": {"protected": "true"},
    "min_count": 0
  }

BEST PRACTICES
--------------

1. Always run retention-plan before retention-apply
2. Keep at least min_count backups regardless of age
3. Ensure min_recovery_window_hours covers your RTO
4. Test restore from oldest retained backup periodically
5. Monitor storage usage and adjust policies as needed

See also: warden help backup, warden help storage
"#;
