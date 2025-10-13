# PostgreSQL Backup Management CLI Guide

This guide covers all the new CLI commands for managing PostgreSQL backups with remote storage and retention policies.

## Prerequisites

Set up your storage credentials as environment variables or pass them as flags:

```bash
export AWS_ACCESS_KEY_ID="your-access-key"
export AWS_SECRET_ACCESS_KEY="your-secret-key"
export AWS_REGION="us-east-1"
export AWS_ENDPOINT="https://s3.amazonaws.com"  # or your MinIO endpoint
```

## Commands Overview

### 1. List Remote Backups

View all backups stored in remote storage with detailed metadata:

```bash
warden postgresql list-backups \
  --remote-storage \
  --storage-bucket my-postgres-backups \
  --storage-provider s3 \
  --storage-region us-east-1
```

**Output:**
```
=== Remote Backups (15) ===

📦 Backup: 550e8400-e29b-41d4-a716-446655440000
   Type: Full
   Status: Completed
   Created: 2025-10-01 14:30:00 UTC
   Duration: 245s
   Size: 12.34 GB
   Server: PostgreSQL 16.1
   Files: 1523 files

📦 Backup: 660e8400-e29b-41d4-a716-446655440001
   Type: Incremental
   Status: Completed
   Created: 2025-10-02 02:00:00 UTC
   Duration: 67s
   Size: 2.15 GB
   Server: PostgreSQL 16.1
   Base Backup: 550e8400-e29b-41d4-a716-446655440000
   Files: 423 files

=== Summary ===
Total backups: 15
Total size: 45.67 GB
```

### 2. Inspect Backup Details

Get comprehensive metadata about a specific backup:

```bash
warden postgresql inspect-backup \
  --backup-id 550e8400-e29b-41d4-a716-446655440000 \
  --storage-bucket my-postgres-backups \
  --storage-provider s3
```

**Output:**
```
=== Backup Metadata ===
ID: 550e8400-e29b-41d4-a716-446655440000
Type: Full
Status: Completed
Start Time: 2025-10-01 14:30:00 UTC
End Time: 2025-10-01 14:34:05 UTC
Duration: 245 seconds
Size: 12345678900 bytes (11.50 GB)
Server Version: PostgreSQL 16.1
WAL Start: 0/1A000028
WAL End: 0/1B000148
Checksum: a1b2c3d4e5f6...
Pinned: false

=== Files (1523) ===
  backup_label (234 bytes)
    Checksum: abcd1234...
  pg_wal/000000010000000000000001 (16777216 bytes)
  base/16384/2619 (8192 bytes)
  ...
```

### 3. Download Backup

Download a backup from remote storage to local directory:

```bash
warden postgresql download-backup \
  --backup-id 550e8400-e29b-41d4-a716-446655440000 \
  --target-dir ./downloaded-backup \
  --storage-bucket my-postgres-backups \
  --storage-provider s3 \
  --verify-checksums
```

**Output:**
```
Downloading backup 550e8400-e29b-41d4-a716-446655440000 to "./downloaded-backup"...
Downloading file: backup_label...
Downloading file: pg_wal/000000010000000000000001...
...
Backup 550e8400-e29b-41d4-a716-446655440000 downloaded successfully to "./downloaded-backup"
```

### 4. Initialize Retention Policy

Upload a retention policy to your storage bucket:

```bash
warden postgresql init-retention-policy \
  --policy-file examples/retention-policy-standard.json \
  --storage-bucket my-postgres-backups \
  --storage-provider s3
```

**Output:**
```
Initializing retention policy from "examples/retention-policy-standard.json"...
Policy version: 1.0
Policy enabled: true
Policy type: IntervalBased
Retention policy saved successfully to bucket my-postgres-backups
```

### 5. Show Current Retention Policy

View the active retention policy for a bucket:

```bash
warden postgresql show-retention-policy \
  --storage-bucket my-postgres-backups \
  --storage-provider s3
```

**Output:**
```
=== Retention Policy ===
{
  "version": "1.0",
  "enabled": true,
  "policy_type": {
    "type": "IntervalBased",
    "intervals": [
      {
        "after_days": 0,
        "keep_count": 30,
        "spacing_days": 1
      },
      ...
    ],
    "minimum_backups": 2,
    "preserve_chains": true
  },
  ...
}
```

### 6. Evaluate Purge (Dry Run)

See which backups would be deleted according to the retention policy:

```bash
warden postgresql purge-plan \
  --storage-bucket my-postgres-backups \
  --storage-provider s3 \
  --format table
```

**Output:**
```
=== Purge Evaluation ===
Timestamp: 2025-10-02 16:00:00 UTC
Total backups: 50
To keep: 35
To delete: 15
Space to free: 45678900000 bytes (42.53 GB)

=== Backups to Delete ===
  🗑️  old-backup-1 (Full) - Outside retention policy window - 5.23 GB
  🗑️  old-backup-2 (Incremental) - Outside retention policy window - 1.45 GB
  🗑️  failed-backup-3 (Full) - Failed backup - 0.00 GB
  ...

=== Backups to Keep ===
  ✅ recent-backup-1 (Full) - Full backup within retention policy
  ✅ recent-backup-2 (Incremental) - Incremental backup within retention policy
  ...
```

**JSON Format:**
```bash
warden postgresql purge-plan \
  --storage-bucket my-postgres-backups \
  --format json
```

**YAML Format:**
```bash
warden postgresql purge-plan \
  --storage-bucket my-postgres-backups \
  --format yaml
```

### 7. Execute Purge

Actually delete backups according to the retention policy:

**Dry Run (Default):**
```bash
warden postgresql purge \
  --storage-bucket my-postgres-backups \
  --storage-provider s3
```

**Output:**
```
⚠️  DRY RUN MODE - No backups will be deleted
Use --apply to actually execute the purge

=== Purge Summary ===
Total backups: 50
To delete: 15
To keep: 35
Space to free: 45678900000 bytes (42.53 GB)

Backups to be deleted:
  - old-backup-1 (Full) - Outside retention policy window
  - old-backup-2 (Incremental) - Outside retention policy window
  ...

⚠️  This was a dry run. Use --apply to actually delete backups.
```

**Apply with Confirmation:**
```bash
warden postgresql purge \
  --storage-bucket my-postgres-backups \
  --storage-provider s3 \
  --apply
```

**Output:**
```
=== Purge Summary ===
Total backups: 50
To delete: 15
To keep: 35
Space to free: 45678900000 bytes (42.53 GB)

Backups to be deleted:
  - old-backup-1 (Full) - Outside retention policy window
  - old-backup-2 (Incremental) - Outside retention policy window
  ...

Are you sure you want to delete 15 backups? (yes/no): yes

Deleting backup old-backup-1: Outside retention policy window
Successfully deleted backup old-backup-1
Deleting backup old-backup-2: Outside retention policy window
Successfully deleted backup old-backup-2
...

=== Purge Report ===
Dry run: false
Total evaluated: 50
Kept: 35
Deleted: 15
Failed: 0
Space freed: 45678900000 bytes (42.53 GB)
Duration: 23 seconds

✅ Purge completed successfully.
```

**Apply without Confirmation:**
```bash
warden postgresql purge \
  --storage-bucket my-postgres-backups \
  --storage-provider s3 \
  --apply \
  --yes
```

## Retention Policy Examples

### Standard Policy (Recommended)

Daily backups for 30 days, weekly for 1 year, monthly for 2 years, yearly for 10 years:

```json
{
  "version": "1.0",
  "enabled": true,
  "policy_type": {
    "type": "IntervalBased",
    "intervals": [
      {"after_days": 0, "keep_count": 30, "spacing_days": 1},
      {"after_days": 30, "keep_count": 52, "spacing_days": 7},
      {"after_days": 365, "keep_count": 24, "spacing_days": 30},
      {"after_days": 730, "keep_count": 10, "spacing_days": 365}
    ],
    "minimum_backups": 2,
    "preserve_chains": true
  },
  "safety": {
    "dry_run_by_default": true,
    "require_confirmation": true,
    "min_successful_backups": 1,
    "preserve_chains": true
  },
  "notifications": {
    "sentry_enabled": true,
    "report_errors": true,
    "report_summary": true
  }
}
```

### Conservative Policy

90 days of daily backups, 1 year of weekly, 3 years of monthly:

```json
{
  "version": "1.0",
  "enabled": true,
  "policy_type": {
    "type": "IntervalBased",
    "intervals": [
      {"after_days": 0, "keep_count": 90, "spacing_days": 1},
      {"after_days": 90, "keep_count": 52, "spacing_days": 7},
      {"after_days": 455, "keep_count": 36, "spacing_days": 30}
    ],
    "minimum_backups": 5,
    "preserve_chains": true
  },
  "safety": {
    "dry_run_by_default": true,
    "require_confirmation": true,
    "min_successful_backups": 3,
    "preserve_chains": true
  },
  "notifications": {
    "sentry_enabled": true,
    "report_errors": true,
    "report_summary": true
  }
}
```

### Aggressive Policy

Keep only 30 days, minimum 3 backups:

```json
{
  "version": "1.0",
  "enabled": true,
  "policy_type": {
    "type": "TimeBased",
    "keep_within_days": 30,
    "keep_minimum": 3
  },
  "safety": {
    "dry_run_by_default": true,
    "require_confirmation": true,
    "min_successful_backups": 1,
    "preserve_chains": true
  },
  "notifications": {
    "sentry_enabled": true,
    "report_errors": true,
    "report_summary": true
  }
}
```

## Environment Variables

All storage credentials can be provided via environment variables:

```bash
export AWS_ACCESS_KEY_ID="your-access-key"
export AWS_SECRET_ACCESS_KEY="your-secret-key"
export AWS_REGION="us-east-1"
export AWS_ENDPOINT="https://s3.amazonaws.com"
```

Or passed as command-line flags:

```bash
--storage-access-key "your-access-key"
--storage-secret-key "your-secret-key"
--storage-region "us-east-1"
--storage-endpoint "https://s3.amazonaws.com"
```

## Common Workflows

### Setup New Backup Bucket

```bash
# 1. Create initial backup with remote storage
warden postgresql full-backup \
  --remote-storage \
  --storage-bucket my-new-backups \
  --storage-provider s3

# 2. Initialize retention policy
warden postgresql init-retention-policy \
  --policy-file examples/retention-policy-standard.json \
  --storage-bucket my-new-backups

# 3. Verify policy
warden postgresql show-retention-policy \
  --storage-bucket my-new-backups
```

### Monthly Purge Maintenance

```bash
# 1. Check what would be deleted
warden postgresql purge-plan \
  --storage-bucket my-backups \
  --format table

# 2. Execute if acceptable
warden postgresql purge \
  --storage-bucket my-backups \
  --apply
```

### Disaster Recovery

```bash
# 1. List available backups
warden postgresql list-backups \
  --remote-storage \
  --storage-bucket my-backups

# 2. Inspect specific backup
warden postgresql inspect-backup \
  --backup-id <backup-id> \
  --storage-bucket my-backups

# 3. Download backup
warden postgresql download-backup \
  --backup-id <backup-id> \
  --target-dir ./recovery \
  --storage-bucket my-backups \
  --verify-checksums

# 4. Restore from downloaded backup
warden postgresql restore-full \
  --backup-id <backup-id> \
  --backup-dir ./recovery \
  --target-dir /var/lib/postgresql/data
```

## Troubleshooting

### Policy Not Found

```bash
# Error: No retention policy found. Use 'init-retention-policy' to create one.

# Solution: Initialize a policy first
warden postgresql init-retention-policy \
  --policy-file examples/retention-policy-standard.json \
  --storage-bucket my-backups
```

### Access Denied

```bash
# Error: Failed to list backups from remote storage: Access Denied

# Solution: Check credentials
export AWS_ACCESS_KEY_ID="correct-key"
export AWS_SECRET_ACCESS_KEY="correct-secret"
```

### Backup Chain Orphaned

The system automatically preserves backup chains when `preserve_chains: true` is set in the retention policy. This ensures incremental backups are not deleted if their parent full backup is being kept.

## Safety Features

1. **Dry-run by Default**: Purge operations default to dry-run mode
2. **Confirmation Prompts**: Require explicit "yes" to proceed with deletions
3. **Minimum Backup Count**: Always keeps minimum number of successful backups
4. **Chain Preservation**: Never orphans incremental backups
5. **Pinned Backups**: Backups marked as pinned are never deleted
6. **Sentry Reporting**: All operations are monitored and reported
