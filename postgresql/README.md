# PostgreSQL Backup and Restore

A Rust library for managing PostgreSQL database backups and restores, inspired by Barman (Backup and Recovery Manager) documentation about best practices for managing backups and restores processes.

## Features

- **Full Backups**: Complete physical backups of PostgreSQL databases using `pg_basebackup`.
- **Incremental Backups**: Efficient backups that only store changes since the last full backup.
- **Snapshot Backups**: Logical backups using `pg_dump` for schema and data.
- **Restore Options**:
  - Full backup restore
  - Incremental backup restore
  - Point-in-time recovery (PITR)
  - Snapshot backup restore
- **Backup Catalog**: Tracking and management of all backups.

## Requirements

- PostgreSQL server (with `pg_basebackup` and `pg_dump` utilities)
- Rust 1.56 or later

## Usage

### Configuration

```rust
use postgresql::{PostgresConfig, PostgresManager};
use std::path::PathBuf;

// Configure PostgreSQL connection
let config = PostgresConfig {
    host: "localhost".to_string(),
    port: 5432,
    database: "postgres".to_string(),
    user: "postgres".to_string(),
    password: Some("postgres".to_string()),
    ssl_mode: None,
};

// Create backup directory
let backup_dir = PathBuf::from("./backups");

// Create PostgreSQL manager
let mut manager = PostgresManager::new(config, backup_dir)?;
```

### Performing Backups

```rust
// Full backup
let full_backup = manager.full_backup().await?;

// Incremental backup
let incremental_backup = manager.incremental_backup().await?;

// Snapshot backup
let snapshot_backup = manager.snapshot_backup().await?;
```

### Listing Backups

```rust
// List all backups
for backup in manager.list_backups() {
    println!("Backup ID: {}, Type: {:?}, Status: {:?}, Time: {}", 
             backup.id, backup.backup_type, backup.status, backup.start_time);
}

// Get the latest full backup
if let Some(latest_full) = manager.get_latest_full_backup() {
    println!("Latest full backup: {}", latest_full.id);
}
```

### Performing Restores

```rust
// Restore from full backup
let restore_dir = PathBuf::from("./restore_full");
let restore = manager.restore_full_backup(&backup_id, restore_dir).await?;

// Restore with incremental backups
let restore_dir = PathBuf::from("./restore_incremental");
let restore = manager.restore_incremental_backup(&full_backup_id, restore_dir).await?;

// Restore to a point in time
let restore_dir = PathBuf::from("./restore_point_in_time");
let target_time = Utc::now(); // Use specific timestamp for recovery
let restore = manager.restore_point_in_time(&full_backup_id, restore_dir, target_time).await?;

// Restore from snapshot backup
let restore_dir = PathBuf::from("./restore_snapshot");
let restore = manager.restore_snapshot_backup(&snapshot_backup_id, restore_dir).await?;

// List contents of a snapshot backup
let contents = manager.list_snapshot_contents(&snapshot_backup_id).await?;
println!("Snapshot contents: {}", contents);
```

## Examples

See the `examples` directory for complete usage examples.

```bash
# Run the backup and restore example
cargo run --example backup_restore
```

## Architecture

This library is structured around several key components:

1. **PostgresManager**: High-level API for managing backups and restores.
2. **Backup Managers**: Specialized managers for different backup types.
3. **Restore Managers**: Specialized managers for different restore scenarios.
4. **Wrappers**: Low-level wrappers around PostgreSQL utilities like `pg_basebackup` and `pg_dump`.
5. **Catalog**: Tracking and management of backup metadata.