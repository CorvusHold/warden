# Storage Library

A standardized storage library for seamless connectivity to multiple S3-compatible storage providers, including AWS S3, Cloudflare R2, and Google Cloud Storage (GCS).

## Features

- **Unified Interface**: Interact with various S3-compatible storage backends using a consistent API
- **Multiple Provider Support**: 
  - AWS S3
  - Cloudflare R2
  - Google Cloud Storage
- **Streaming Support**: Efficiently upload and download large files with streaming capabilities
- **PostgreSQL Backup Integration**: Seamlessly integrate with PostgreSQL backup and restore operations
- **Comprehensive Testing**: Includes integration tests to ensure reliability with real storage services
- **Detailed Documentation**: Provides guides and examples for common use cases

## Installation

Add the following to your `Cargo.toml`:

```toml
[dependencies]
storage = { path = "../storage" }
```

## Usage

### Basic Usage

```rust
use storage::{StorageProviderFactory, StorageProviderType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an S3 provider
    let s3_provider = StorageProviderFactory::create_s3_provider(
        Some("us-west-2".to_string()),  // Region
        None,                           // Endpoint (None for AWS)
        std::env::var("AWS_ACCESS_KEY_ID").ok(),
        std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
    ).await?;
    
    // Create a bucket
    s3_provider.create_bucket("my-bucket").await?;
    
    // Upload a file
    s3_provider.upload_file(
        "my-bucket",
        "path/to/object.txt",
        std::path::Path::new("/local/path/to/file.txt"),
        Some("text/plain"),
        None,
    ).await?;
    
    // List objects in a bucket
    let objects = s3_provider.list_objects("my-bucket", None).await?;
    for object in objects {
        println!("Object: {}", object.key);
    }
    
    Ok(())
}
```

### PostgreSQL Backup Integration

The library provides a dedicated `PostgresBackupStorage` class for integrating with PostgreSQL backup systems. It supports both physical backups (using `pg_basebackup`) and logical backups (using `pg_dump`), handling the storage and retrieval of these backups in S3-compatible storage services.

```rust
use storage::{PostgresBackupStorage, StorageProviderType};
use std::path::Path;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a PostgreSQL backup storage instance
    let storage = PostgresBackupStorage::new(
        StorageProviderType::S3,
        "postgres-backups".to_string(),
        Some("backups".to_string()),
        Some("us-west-2".to_string()),
        None,
        std::env::var("AWS_ACCESS_KEY_ID").ok(),
        std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
        None,
        None,
        None,
    ).await?;
    
    // Upload a backup directory
    let backup_id = "backup-2023-06-15";
    let backup_path = Path::new("/path/to/backup/directory");
    storage.upload_backup(backup_id, backup_path, None).await?;
    
    // List all backups
    let backups = storage.list_backups().await?;
    for backup in backups {
        println!("Backup: {}", backup);
    }
    
    // Generate a pre-signed URL for a backup file (e.g., for sharing)
    let url = storage
        .generate_backup_file_url(backup_id, "base.tar.gz", Duration::from_secs(3600))
        .await?;
    println!("Pre-signed URL: {}", url);
    
    // Download a backup
    let target_dir = Path::new("/path/to/restore/directory");
    storage.download_backup(backup_id, target_dir).await?;
    
    // Delete a backup when no longer needed
    storage.delete_backup(backup_id).await?;
    
    Ok(())
}
```

#### Backup Structure

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

#### Complete Workflow Example

See the `postgres_backup_workflow.rs` example for a complete workflow that demonstrates:

1. Creating a PostgreSQL backup (both physical and logical)
2. Uploading it to S3-compatible storage
3. Downloading the backup
4. Restoring the PostgreSQL database

## Provider-Specific Configuration

### AWS S3

```rust
let s3_provider = StorageProviderFactory::create_s3_provider(
    Some("us-west-2".to_string()),
    None,  // Use None for standard AWS endpoints
    std::env::var("AWS_ACCESS_KEY_ID").ok(),
    std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
).await?;
```

### Cloudflare R2

```rust
let r2_provider = StorageProviderFactory::create_r2_provider(
    "your-account-id".to_string(),
    "your-access-key".to_string(),
    "your-secret-key".to_string(),
).await?;
```

### Google Cloud Storage

```rust
let gcs_provider = StorageProviderFactory::create_gcs_provider(
    "your-project-id".to_string(),
    Some("/path/to/credentials.json".to_string()),
).await?;
```

Alternatively, you can use environment variables for GCS:

```bash
export GCS_ACCESS_KEY=your-access-key
export GCS_SECRET_KEY=your-secret-key
```

```rust
let gcs_provider = StorageProviderFactory::create_gcs_provider(
    "your-project-id".to_string(),
    None,  // Will use environment variables
).await?;
```

## Documentation

Detailed documentation is available in the `docs/` directory:

- [PostgreSQL Integration Guide](docs/postgres_integration_guide.md): Comprehensive guide for integrating PostgreSQL backups with the storage library

## Testing

The library includes integration tests that verify functionality with real storage services:

```bash
# Run the PostgreSQL integration test (requires S3 credentials)
cargo test --test postgres_integration_test -- --ignored
```

## Streaming Support

For large files, you can use streaming uploads and downloads:

```rust
use tokio::fs::File;
use tokio_util::io::ReaderStream;

// Upload a file as a stream
let file = File::open("/path/to/large/file.tar").await?;
let stream = ReaderStream::new(file);

provider.upload_stream(
    "my-bucket",
    "path/to/object.tar",
    stream,
    Some("application/x-tar"),
    None,
).await?;
```

## Error Handling

The library provides a comprehensive `StorageError` type for handling errors:

```rust
use storage::StorageError;

match result {
    Ok(_) => println!("Operation succeeded"),
    Err(StorageError::NotFound(msg)) => println!("Not found: {}", msg),
    Err(StorageError::AlreadyExists(msg)) => println!("Already exists: {}", msg),
    Err(StorageError::AccessDenied(msg)) => println!("Access denied: {}", msg),
    Err(StorageError::Configuration(msg)) => println!("Configuration error: {}", msg),
    Err(StorageError::Io(err)) => println!("IO error: {}", err),
    Err(StorageError::Unexpected(msg)) => println!("Unexpected error: {}", msg),
}
```

## Testing

To run the integration tests with MinIO:

1. Start a local MinIO server:

```bash
docker run -p 9000:9000 -p 9001:9001 \
  -e "MINIO_ROOT_USER=minioadmin" \
  -e "MINIO_ROOT_PASSWORD=minioadmin" \
  minio/minio server /data --console-address ":9001"
```

2. Set the environment variables for testing:

```bash
export STORAGE_TEST_MINIO_ENDPOINT=http://localhost:9000
export STORAGE_TEST_MINIO_ACCESS_KEY=minioadmin
export STORAGE_TEST_MINIO_SECRET_KEY=minioadmin
```

3. Run the tests:

```bash
cargo test
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.
