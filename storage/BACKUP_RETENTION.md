# Backup Metadata & Retention Policy System

This document describes the backup metadata tracking and retention policy system implemented in the storage module.

## Overview

The system provides:
1. **Detailed backup metadata** tracked in remote storage
2. **Flexible retention policies** stored at the bucket level
3. **Safe purge evaluation** with dry-run support
4. **Backup chain preservation** to avoid orphaning incremental backups
5. **Sentry integration** for monitoring and error reporting

## Backup Metadata

Each backup stored in remote storage includes a `backup_metadata.json` file with:

```json
{
  "id": "uuid",
  "backup_type": "Full|Incremental|Snapshot",
  "status": "Completed",
  "start_time": "2025-10-02T14:00:00Z",
  "end_time": "2025-10-02T14:30:00Z",
  "base_backup_id": "parent-uuid",
  "wal_start": "0/1000000",
  "wal_end": "0/2000000",
  "size_bytes": 1234567890,
  "server_version": "16.1",
  "checksum": "sha256:...",
  "files": [
    {
      "name": "backup_label",
      "size": 1024,
      "checksum": "sha256:..."
    }
  ],
  "tags": ["production", "pre-migration"],
  "pinned": false
}
```

### Features
- **SHA256 checksums** for files < 100MB
- **File inventory** with sizes
- **Tags** for policy exceptions
- **Pinning** to prevent deletion
- **WAL positions** for point-in-time recovery

## Retention Policies

Policies are stored as `retention_policy.json` at the bucket root (per-bucket configuration).

### Policy Types

#### 1. Time-Based Retention
Keep backups within a time window + minimum count.

```json
{
  "version": "1.0",
  "enabled": true,
  "policy_type": {
    "type": "TimeBased",
    "keep_within_days": 30,
    "keep_minimum": 5
  },
  "safety": {
    "dry_run_by_default": true,
    "require_confirmation": true,
    "min_successful_backups": 2,
    "preserve_chains": true
  },
  "notifications": {
    "sentry_enabled": true,
    "report_errors": true,
    "report_summary": true
  }
}
```

#### 2. Count-Based Retention
Keep N most recent backups per type.

```json
{
  "policy_type": {
    "type": "CountBased",
    "max_full_backups": 10,
    "max_incrementals_per_full": 20,
    "keep_latest": 3
  }
}
```

#### 3. Interval-Based Retention (Recommended)
Keep backups at different intervals (daily/weekly/monthly/yearly).

```json
{
  "policy_type": {
    "type": "IntervalBased",
    "intervals": [
      {
        "after_days": 0,
        "keep_count": 30,
        "spacing_days": 1
      },
      {
        "after_days": 30,
        "keep_count": 52,
        "spacing_days": 7
      },
      {
        "after_days": 365,
        "keep_count": 24,
        "spacing_days": 30
      },
      {
        "after_days": 730,
        "keep_count": 10,
        "spacing_days": 365
      }
    ],
    "minimum_backups": 2,
    "preserve_chains": true
  }
}
```

**This gives you:**
- **0-30 days**: Daily backups (30 total)
- **30-365 days**: Weekly backups (52 total)
- **1-2 years**: Monthly backups (24 total)
- **2+ years**: Yearly backups (10 total)

## Safety Features

### 1. Dry-Run Mode
All purge operations default to dry-run, showing what would be deleted without actually deleting anything.

### 2. Minimum Backup Count
Always keeps at least N successful backups regardless of policy rules.

### 3. Chain Preservation
When enabled, keeps all incremental backups that depend on full backups being kept.

### 4. Pinned Backups
Backups marked as `pinned: true` are never deleted.

### 5. Confirmation Required
Can be configured to require explicit user confirmation before deletion.

## API Methods

### PostgresBackupStorage

```rust
// Create and upload backup with metadata
pub async fn create_backup_metadata(...) -> Result<BackupMetadata, StorageError>;
pub async fn upload_backup_metadata(...) -> Result<(), StorageError>;

// List and retrieve remote backups
pub async fn list_remote_backups_detailed() -> Result<Vec<BackupMetadata>, StorageError>;
pub async fn get_remote_backup_metadata(backup_id: &str) -> Result<BackupMetadata, StorageError>;

// Retention policy management
pub async fn load_retention_policy() -> Result<Option<RetentionPolicy>, StorageError>;
pub async fn save_retention_policy(policy: &RetentionPolicy) -> Result<(), StorageError>;

// Purge operations
pub async fn evaluate_purge(policy: &RetentionPolicy) -> Result<PurgeEvaluation, StorageError>;
pub async fn execute_purge(evaluation: &PurgeEvaluation, dry_run: bool) -> Result<PurgeReport, StorageError>;
```

## Storage Structure

```
s3://bucket/prefix/
├── retention_policy.json           # Bucket-level policy
├── {backup-uuid-1}/
│   ├── backup_metadata.json        # Backup metadata
│   ├── backup_label
│   ├── pg_dump.dump
│   └── ...
├── {backup-uuid-2}/
│   ├── backup_metadata.json
│   └── ...
```

## Purge Evaluation Output

```rust
PurgeEvaluation {
    timestamp: DateTime<Utc>,
    total_backups: 100,
    to_keep: Vec<BackupPurgeDecision>,      // Backups to keep
    to_delete: Vec<BackupPurgeDecision>,    // Backups to delete
    warnings: Vec<String>,                   // Policy warnings
    estimated_space_freed: 12345678900,      // Bytes
}

BackupPurgeDecision {
    backup_id: String,
    backup_type: BackupType,
    timestamp: DateTime<Utc>,
    size_bytes: u64,
    reason: String,                          // Why kept/deleted
    pinned: bool,
    has_dependents: bool,                    // Has incremental children
}
```

## Purge Report

```rust
PurgeReport {
    timestamp: DateTime<Utc>,
    dry_run: bool,
    total_evaluated: 100,
    kept: 70,
    deleted: 30,
    failed: 0,
    space_freed: 12345678900,               // Actual bytes freed
    duration_secs: 45,
    errors: Vec<String>,
}
```

## Sentry Integration

When enabled, the system reports:
- **Errors**: Failed deletions
- **Summaries**: Purge operation results (backups deleted, space freed)
- **Breadcrumbs**: Operation tracking

## Next Steps

To complete the implementation, we still need to:

1. **CLI Commands** (in `postgres` module):
   - `list-backups --source remote` - List remote backups
   - `inspect-backup <id>` - Show backup details
   - `download-backup <id>` - Download backup from remote
   - `init-retention-policy` - Initialize policy for bucket
   - `show-retention-policy` - Display current policy
   - `purge-plan` - Evaluate purge (dry run)
   - `purge --apply` - Execute purge

2. **Integration with PostgresManager**:
   - Add `storage: Option<PostgresBackupStorage>` field
   - Automatically upload metadata after backup
   - Sync catalog with remote storage

3. **Testing**:
   - Unit tests for policy evaluation
   - Integration tests with MinIO
   - Test chain preservation logic

4. **Documentation**:
   - CLI usage examples
   - Policy configuration guide
   - Migration guide for existing backups

## Example Workflow

```bash
# 1. Create a backup and upload to remote
warden postgres full-backup --remote-storage --storage-bucket backups

# 2. List all remote backups
warden postgres list-backups --source remote --storage-bucket backups

# 3. Initialize retention policy
warden postgres init-retention-policy \
  --storage-bucket backups \
  --policy-file my-policy.json

# 4. Evaluate what would be deleted (dry run)
warden postgres purge-plan --storage-bucket backups

# 5. Execute purge
warden postgres purge --storage-bucket backups --apply

# 6. Download a specific backup
warden postgres download-backup <backup-id> \
  --target-dir ./restore \
  --storage-bucket backups
```

## Configuration Examples

### Conservative Policy
Keeps 90 days of daily backups, 1 year of weekly, 3 years of monthly:

```json
{
  "policy_type": {
    "type": "IntervalBased",
    "intervals": [
      {"after_days": 0, "keep_count": 90, "spacing_days": 1},
      {"after_days": 90, "keep_count": 52, "spacing_days": 7},
      {"after_days": 455, "keep_count": 36, "spacing_days": 30}
    ],
    "minimum_backups": 5,
    "preserve_chains": true
  }
}
```

### Aggressive Policy
Keeps only 30 days with minimum 3 backups:

```json
{
  "policy_type": {
    "type": "TimeBased",
    "keep_within_days": 30,
    "keep_minimum": 3
  }
}
```
