# PostgreSQL Backup Integration Guide

This guide explains how to use the storage library's PostgreSQL backup integration to store and retrieve PostgreSQL database backups in S3-compatible storage services.

## Overview

The storage library provides a `PostgresBackupStorage` struct that offers a high-level interface for managing PostgreSQL backups in cloud storage. It supports:

- Uploading both physical and logical backups to S3-compatible storage
- Downloading backups for restoration
- Listing available backups
- Generating pre-signed URLs for backup files
- Deleting backups when no longer needed

## Prerequisites

Before using the PostgreSQL backup integration, ensure you have:

1. A PostgreSQL database server (local or remote)
2. Access to an S3-compatible storage service (AWS S3, Cloudflare R2, Google Cloud Storage, etc.)
3. Appropriate credentials for the storage service

## Installation

Add the storage library to your `Cargo.toml`:

```toml
[dependencies]
storage = { path = "../storage" }
tokio = { version = "1", features = ["full"] }
```

## Basic Usage

### Creating a PostgresBackupStorage Instance

```rust
use storage::{PostgresBackupStorage, StorageProviderType};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a PostgreSQL backup storage instance
    let storage = PostgresBackupStorage::new(
        StorageProviderType::S3,
        "postgres-backups".to_string(),
        None, // No prefix
        Some(env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string())),
        None, // Use default AWS S3 endpoint
        env::var("AWS_ACCESS_KEY_ID").ok(),
        env::var("AWS_SECRET_ACCESS_KEY").ok(),
        None, // No project ID (only needed for GCS)
        None, // No account ID (only needed for Cloudflare R2)
        None, // No additional options
    ).await?;

    // Use the storage instance...

    Ok(())
}
```

### Uploading a Backup

```rust
use std::path::Path;

async fn upload_example(storage: &PostgresBackupStorage) -> Result<(), Box<dyn std::error::Error>> {
    let backup_id = "backup-20230615-120000";
    let backup_dir = Path::new("/path/to/backup/directory");
    
    // Upload the backup
    storage.upload_backup(backup_id, backup_dir, None).await?;
    
    info!("Backup uploaded successfully: {}", backup_id);
    Ok(())
}
```

### Listing Available Backups

```rust
async fn list_backups_example(storage: &PostgresBackupStorage) -> Result<(), Box<dyn std::error::Error>> {
    let backups = storage.list_backups().await?;
    
    info!("Available backups:");
    for (i, backup) in backups.iter().enumerate() {
        info!("{}. {}", i + 1, backup);
    }
    
    Ok(())
}
```

### Generating a Pre-signed URL

```rust
use std::time::Duration;

async fn generate_url_example(storage: &PostgresBackupStorage, backup_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Generate a pre-signed URL for the backup metadata file
    let url = storage
        .generate_backup_file_url(backup_id, "backup_metadata.json", Duration::from_secs(3600))
        .await?;
    
    info!("Pre-signed URL (valid for 1 hour): {}", url);
    Ok(())
}
```

### Downloading a Backup

```rust
use std::path::Path;

async fn download_example(storage: &PostgresBackupStorage, backup_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let restore_dir = Path::new("/path/to/restore/directory");
    
    // Download the backup
    storage.download_backup(backup_id, restore_dir).await?;
    
    info!("Backup downloaded successfully to: {}", restore_dir.display());
    Ok(())
}
```

### Deleting a Backup

```rust
async fn delete_example(storage: &PostgresBackupStorage, backup_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Delete the backup
    storage.delete_backup(backup_id).await?;
    
    info!("Backup deleted successfully: {}", backup_id);
    Ok(())
}
```

## Complete Backup and Restore Workflow

For a complete workflow that demonstrates creating, uploading, and restoring a PostgreSQL backup, refer to the `postgres_backup_workflow.rs` example included with the library.

### Running the Example

```bash
# Set required environment variables
export AWS_REGION=us-west-2
export AWS_ACCESS_KEY_ID=your_access_key
export AWS_SECRET_ACCESS_KEY=your_secret_key
export BUCKET_NAME=postgres-backups

# Run the example
cargo run --example postgres_backup_workflow
```

## Backup Structure

A PostgreSQL backup typically includes:

1. **Physical backup files**:
   - `base.tar.gz` - The base backup created by `pg_basebackup`
   - `pg_wal/` - Directory containing WAL (Write-Ahead Log) files

2. **Logical backup files**:
   - `pg_dump.sql` - Plain SQL dump created by `pg_dump`
   - `pg_dump.dump` - Custom format dump created by `pg_dump`

3. **Metadata**:
   - `backup_metadata.json` - Contains information about the backup, such as:
     - Backup ID
     - Backup type (full, incremental, snapshot)
     - Start and end times
     - Database version
     - Database size
     - WAL position

## Integration with PostgreSQL Backup System

The storage library is designed to work seamlessly with the PostgreSQL backup system. Here's how to integrate it with your existing backup process:

### 1. Create PostgreSQL Backups

First, create physical and logical backups using the PostgreSQL tools:

```rust
use std::process::Command;
use std::path::Path;

async fn create_postgres_backup(backup_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Create physical backup using pg_basebackup
    let pg_basebackup_output = Command::new("pg_basebackup")
        .args([
            "-D", backup_dir.join("base").to_str().unwrap(),
            "-Ft", // Output in tar format
            "-z", // Compress with gzip
            "-h", "localhost",
            "-p", "5432",
            "-U", "postgres",
        ])
        .output()?;
    
    if !pg_basebackup_output.status.success() {
        return Err(format!("pg_basebackup failed: {}", 
            String::from_utf8_lossy(&pg_basebackup_output.stderr)).into());
    }
    
    // Create logical backup using pg_dump
    let pg_dump_output = Command::new("pg_dump")
        .args([
            "-h", "localhost",
            "-p", "5432",
            "-U", "postgres",
            "-d", "postgres",
            "-f", backup_dir.join("pg_dump.sql").to_str().unwrap(),
        ])
        .output()?;
    
    if !pg_dump_output.status.success() {
        return Err(format!("pg_dump failed: {}", 
            String::from_utf8_lossy(&pg_dump_output.stderr)).into());
    }
    
    // Create custom format dump
    let pg_dump_custom_output = Command::new("pg_dump")
        .args([
            "-h", "localhost",
            "-p", "5432",
            "-U", "postgres",
            "-d", "postgres",
            "-Fc", // Custom format
            "-f", backup_dir.join("pg_dump.dump").to_str().unwrap(),
        ])
        .output()?;
    
    if !pg_dump_custom_output.status.success() {
        return Err(format!("pg_dump custom format failed: {}", 
            String::from_utf8_lossy(&pg_dump_custom_output.stderr)).into());
    }
    
    Ok(())
}
```

### 2. Create Backup Metadata

Create a metadata file to store information about the backup:

```rust
use serde::{Serialize, Deserialize};
use std::fs::File;
use std::io::Write;
use chrono::Utc;

#[derive(Serialize, Deserialize)]
struct BackupMetadata {
    backup_id: String,
    backup_type: String,
    start_time: String,
    end_time: String,
    database_name: String,
    database_version: String,
    database_size: u64,
    wal_position: String,
    files: Vec<BackupFile>,
}

#[derive(Serialize, Deserialize)]
struct BackupFile {
    name: String,
    size: u64,
    checksum: String,
}

async fn create_backup_metadata(backup_dir: &Path, backup_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Get PostgreSQL version
    let version_output = Command::new("psql")
        .args([
            "-h", "localhost",
            "-p", "5432",
            "-U", "postgres",
            "-c", "SELECT version();",
            "-t", // Tuple only output
        ])
        .output()?;
    
    let version = String::from_utf8_lossy(&version_output.stdout)
        .trim()
        .to_string();
    
    // Get database size
    let size_output = Command::new("psql")
        .args([
            "-h", "localhost",
            "-p", "5432",
            "-U", "postgres",
            "-c", "SELECT pg_database_size('postgres');",
            "-t", // Tuple only output
        ])
        .output()?;
    
    let size: u64 = String::from_utf8_lossy(&size_output.stdout)
        .trim()
        .parse()?;
    
    // Get WAL position
    let wal_output = Command::new("psql")
        .args([
            "-h", "localhost",
            "-p", "5432",
            "-U", "postgres",
            "-c", "SELECT pg_current_wal_lsn();",
            "-t", // Tuple only output
        ])
        .output()?;
    
    let wal_position = String::from_utf8_lossy(&wal_output.stdout)
        .trim()
        .to_string();
    
    // Create metadata
    let metadata = BackupMetadata {
        backup_id: backup_id.to_string(),
        backup_type: "full".to_string(),
        start_time: Utc::now().to_rfc3339(),
        end_time: Utc::now().to_rfc3339(),
        database_name: "postgres".to_string(),
        database_version: version,
        database_size: size,
        wal_position,
        files: Vec::new(), // You would populate this with actual file information
    };
    
    // Write metadata to file
    let metadata_json = serde_json::to_string_pretty(&metadata)?;
    let mut file = File::create(backup_dir.join("backup_metadata.json"))?;
    file.write_all(metadata_json.as_bytes())?;
    
    Ok(())
}
```

### 3. Upload Backup to Storage

Use the `PostgresBackupStorage` to upload the backup:

```rust
async fn upload_to_storage(backup_dir: &Path, backup_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = PostgresBackupStorage::new(
        StorageProviderType::S3,
        "postgres-backups".to_string(),
        None,
        Some(env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string())),
        None,
        env::var("AWS_ACCESS_KEY_ID").ok(),
        env::var("AWS_SECRET_ACCESS_KEY").ok(),
        None,
        None,
        None,
    ).await?;
    
    storage.upload_backup(backup_id, backup_dir, None).await?;
    
    Ok(())
}
```

### 4. Restore from Backup

To restore from a backup:

```rust
async fn restore_from_backup(backup_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let storage = PostgresBackupStorage::new(
        StorageProviderType::S3,
        "postgres-backups".to_string(),
        None,
        Some(env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string())),
        None,
        env::var("AWS_ACCESS_KEY_ID").ok(),
        env::var("AWS_SECRET_ACCESS_KEY").ok(),
        None,
        None,
        None,
    ).await?;
    
    // Download the backup
    let restore_dir = Path::new("/tmp/postgres_restore");
    storage.download_backup(backup_id, restore_dir).await?;
    
    // Stop PostgreSQL server
    Command::new("pg_ctl")
        .args([
            "-D", "/usr/local/var/postgres", // Adjust to your PostgreSQL data directory
            "stop",
        ])
        .output()?;
    
    // Restore physical backup
    Command::new("tar")
        .args([
            "-xzf", restore_dir.join("base.tar.gz").to_str().unwrap(),
            "-C", "/usr/local/var/postgres", // Adjust to your PostgreSQL data directory
        ])
        .output()?;
    
    // Start PostgreSQL server
    Command::new("pg_ctl")
        .args([
            "-D", "/usr/local/var/postgres", // Adjust to your PostgreSQL data directory
            "start",
        ])
        .output()?;
    
    // Restore logical backup if needed
    Command::new("psql")
        .args([
            "-h", "localhost",
            "-p", "5432",
            "-U", "postgres",
            "-d", "postgres",
            "-f", restore_dir.join("pg_dump.sql").to_str().unwrap(),
        ])
        .output()?;
    
    Ok(())
}
```

## Error Handling

The PostgreSQL backup integration provides comprehensive error handling through the `StorageError` type:

```rust
use storage::error::StorageError;

async fn handle_errors() -> Result<(), StorageError> {
    let storage = PostgresBackupStorage::new(/* ... */).await?;
    
    match storage.upload_backup("backup-id", &Path::new("/path/to/backup"), None).await {
        Ok(_) => info!("Backup uploaded successfully"),
        Err(StorageError::BucketNotFound(bucket)) => {
            info!("Bucket '{}' not found", bucket);
            // Create the bucket and retry
        },
        Err(StorageError::ObjectNotFound { bucket, key }) => {
            info!("Object '{}' not found in bucket '{}'", key, bucket);
        },
        Err(e) => info!("Error: {}", e),
    }
    
    Ok(())
}
```

## Best Practices

1. **Backup Naming**: Use a consistent naming scheme for backups, such as `backup-YYYYMMDD-HHMMSS` to make them easily identifiable.

2. **Regular Backups**: Schedule regular backups to ensure data safety.

3. **Backup Rotation**: Implement a backup rotation policy to delete old backups and save storage costs.

4. **Encryption**: Consider encrypting sensitive backup data before uploading.

5. **Testing Restores**: Regularly test the restore process to ensure backups are valid.

6. **Monitoring**: Monitor the backup process and set up alerts for failures.

7. **Documentation**: Document your backup and restore procedures for operational use.

## Conclusion

The PostgreSQL backup integration in the storage library provides a robust solution for storing and retrieving PostgreSQL backups in S3-compatible storage services. By following this guide, you can implement a comprehensive backup and restore strategy for your PostgreSQL databases.

For more examples and detailed API documentation, refer to the library's examples and API documentation.
