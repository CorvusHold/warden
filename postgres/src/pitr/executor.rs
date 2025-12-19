//! PITR recovery execution.

use chrono::Utc;
use log::{debug, error, info, warn};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

use storage::PostgresBackupStorage;

use crate::PostgresError;

use super::types::{PitrDetails, PitrResult, PitrStatus, RecoveryPlan, RecoveryTarget};

/// PITR executor for performing point-in-time recovery.
pub struct PitrExecutor {
    /// The recovery plan to execute.
    plan: RecoveryPlan,
    /// Target directory for recovery.
    target_dir: PathBuf,
    /// Optional remote storage for downloading backups/WAL.
    storage: Option<PostgresBackupStorage>,
    /// Local backup directory (for local backups).
    backup_dir: Option<PathBuf>,
    /// Whether to start PostgreSQL after recovery.
    auto_start: bool,
    /// PostgreSQL binary directory (optional).
    pg_bin_dir: Option<PathBuf>,
}

impl PitrExecutor {
    /// Create a new PITR executor.
    pub fn new(plan: RecoveryPlan, target_dir: PathBuf) -> Self {
        Self {
            plan,
            target_dir,
            storage: None,
            backup_dir: None,
            auto_start: false,
            pg_bin_dir: None,
        }
    }

    /// Set remote storage for downloading.
    pub fn with_storage(mut self, storage: PostgresBackupStorage) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Set local backup directory.
    pub fn with_backup_dir(mut self, dir: PathBuf) -> Self {
        self.backup_dir = Some(dir);
        self
    }

    /// Enable auto-start after recovery.
    pub fn with_auto_start(mut self, auto_start: bool) -> Self {
        self.auto_start = auto_start;
        self
    }

    /// Set PostgreSQL binary directory.
    pub fn with_pg_bin_dir(mut self, dir: PathBuf) -> Self {
        self.pg_bin_dir = Some(dir);
        self
    }

    /// Execute the recovery plan.
    pub async fn execute(&self) -> Result<PitrResult, PostgresError> {
        let started_at = Utc::now();
        let mut details = PitrDetails::default();

        info!("Starting PITR execution for plan {}", self.plan.id);
        info!("Target directory: {:?}", self.target_dir);
        info!("Recovery target: {:?}", self.plan.target);

        // Validate plan is still valid
        if !self.plan.validation.is_valid {
            return Err(PostgresError::RestoreError(format!(
                "Recovery plan is invalid: {}",
                self.plan.validation.errors.join("; ")
            )));
        }

        // Step 1: Prepare target directory
        self.prepare_target_directory()?;

        // Step 2: Restore base backup
        let download_start = std::time::Instant::now();
        self.restore_base_backup().await?;
        details.bytes_downloaded += self.plan.base_backup.size_bytes;

        // Step 3: Download and stage WAL segments
        self.stage_wal_segments(&mut details).await?;
        details.download_duration_secs = download_start.elapsed().as_secs();

        // Step 4: Configure recovery
        let apply_start = std::time::Instant::now();
        self.configure_recovery()?;
        details.recovery_mode = self.get_recovery_mode();

        // Step 5: Start PostgreSQL in recovery mode (if auto_start)
        if self.auto_start {
            self.start_recovery()?;
            details.apply_duration_secs = apply_start.elapsed().as_secs();
        } else {
            info!("Auto-start disabled. To complete recovery:");
            info!(
                "  1. Start PostgreSQL with data directory: {:?}",
                self.target_dir
            );
            info!("  2. PostgreSQL will replay WAL and stop at the target");
            info!("  3. After recovery, promote to primary or restart normally");
        }

        let result = PitrResult {
            id: Uuid::new_v4(),
            plan_id: self.plan.id,
            started_at,
            completed_at: Some(Utc::now()),
            status: PitrStatus::Completed,
            target_dir: self.target_dir.clone(),
            error_message: None,
            details,
        };

        info!("PITR execution completed: {}", result.id);
        Ok(result)
    }

    /// Prepare the target directory for recovery.
    fn prepare_target_directory(&self) -> Result<(), PostgresError> {
        info!("Preparing target directory: {:?}", self.target_dir);

        if self.target_dir.exists() {
            // Check if directory is empty
            let entries: Vec<_> = fs::read_dir(&self.target_dir)
                .map_err(PostgresError::Io)?
                .collect();

            if !entries.is_empty() {
                warn!("Target directory is not empty, clearing contents");
                for entry in entries {
                    let entry = entry.map_err(PostgresError::Io)?;
                    let path = entry.path();
                    if path.is_dir() {
                        fs::remove_dir_all(&path).map_err(PostgresError::Io)?;
                    } else {
                        fs::remove_file(&path).map_err(PostgresError::Io)?;
                    }
                }
            }
        } else {
            fs::create_dir_all(&self.target_dir).map_err(PostgresError::Io)?;
        }

        // Set appropriate permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.target_dir, fs::Permissions::from_mode(0o700))
                .map_err(PostgresError::Io)?;
        }

        Ok(())
    }

    /// Restore the base backup to the target directory.
    async fn restore_base_backup(&self) -> Result<(), PostgresError> {
        info!("Restoring base backup: {}", self.plan.base_backup.id);

        if self.plan.base_backup.is_remote {
            // Download from remote storage
            let storage = self.storage.as_ref().ok_or_else(|| {
                PostgresError::RestoreError(
                    "Remote storage required but not configured".to_string(),
                )
            })?;

            storage
                .download_backup(&self.plan.base_backup.id.to_string(), &self.target_dir)
                .await
                .map_err(|e| {
                    PostgresError::RestoreError(format!("Failed to download backup: {}", e))
                })?;
        } else {
            // Copy from local backup
            let backup_path = Path::new(&self.plan.base_backup.path);
            if !backup_path.exists() {
                return Err(PostgresError::RestoreError(format!(
                    "Local backup not found: {:?}",
                    backup_path
                )));
            }

            Self::copy_directory(backup_path, &self.target_dir)?;
        }

        info!("Base backup restored successfully");
        Ok(())
    }

    /// Copy a directory recursively.
    fn copy_directory(src: &Path, dst: &Path) -> Result<(), PostgresError> {
        if !dst.exists() {
            fs::create_dir_all(dst).map_err(PostgresError::Io)?;
        }

        for entry in fs::read_dir(src).map_err(PostgresError::Io)? {
            let entry = entry.map_err(PostgresError::Io)?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                Self::copy_directory(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path).map_err(PostgresError::Io)?;
            }
        }

        Ok(())
    }

    /// Stage WAL segments for recovery.
    async fn stage_wal_segments(&self, details: &mut PitrDetails) -> Result<(), PostgresError> {
        let wal_dir = self.target_dir.join("pg_wal");
        if !wal_dir.exists() {
            fs::create_dir_all(&wal_dir).map_err(PostgresError::Io)?;
        }

        info!("Staging {} WAL segments", self.plan.wal_segments.len());

        for segment in &self.plan.wal_segments {
            let target_path = wal_dir.join(&segment.filename);

            if segment.is_remote {
                // Download from remote storage
                if let Some(storage) = &self.storage {
                    // Extract backup_id and filename from the path
                    let parts: Vec<&str> = segment.path.split('/').collect();
                    if parts.len() >= 2 {
                        let backup_id = parts[parts.len() - 2];
                        let filename = parts[parts.len() - 1];

                        storage
                            .download_backup_file(backup_id, filename, &target_path)
                            .await
                            .map_err(|e| {
                                PostgresError::WalError(format!(
                                    "Failed to download WAL segment {}: {}",
                                    segment.filename, e
                                ))
                            })?;

                        details.wal_segments_downloaded += 1;
                        details.bytes_downloaded += segment.size_bytes;
                    }
                }
            } else {
                // Copy from local path
                let src_path = Path::new(&segment.path);
                if src_path.exists() {
                    fs::copy(src_path, &target_path).map_err(PostgresError::Io)?;
                } else {
                    warn!("Local WAL segment not found: {:?}", src_path);
                }
            }

            // Decompress if needed
            if segment.is_compressed {
                self.decompress_wal_segment(&target_path)?;
            }
        }

        info!("WAL segments staged successfully");
        Ok(())
    }

    /// Decompress a WAL segment.
    fn decompress_wal_segment(&self, path: &Path) -> Result<(), PostgresError> {
        let path_str = path.to_string_lossy();

        if path_str.ends_with(".gz") {
            let output_path = path.with_extension("");
            let status = Command::new("gunzip")
                .arg("-k") // Keep original
                .arg(path)
                .status()
                .map_err(|e| PostgresError::WalError(format!("Failed to decompress: {}", e)))?;

            if !status.success() {
                return Err(PostgresError::WalError(format!(
                    "gunzip failed for {:?}",
                    path
                )));
            }

            // Remove compressed file
            fs::remove_file(path).map_err(PostgresError::Io)?;
            debug!("Decompressed WAL segment: {:?}", output_path);
        } else if path_str.ends_with(".lz4") {
            let output_path = path.with_extension("");
            let status = Command::new("lz4")
                .arg("-d")
                .arg("-f")
                .arg(path)
                .arg(&output_path)
                .status()
                .map_err(|e| PostgresError::WalError(format!("Failed to decompress: {}", e)))?;

            if !status.success() {
                return Err(PostgresError::WalError(format!(
                    "lz4 decompression failed for {:?}",
                    path
                )));
            }

            fs::remove_file(path).map_err(PostgresError::Io)?;
            debug!("Decompressed WAL segment: {:?}", output_path);
        }

        Ok(())
    }

    /// Configure PostgreSQL for recovery.
    fn configure_recovery(&self) -> Result<(), PostgresError> {
        info!("Configuring PostgreSQL for recovery");

        // Determine PostgreSQL version to use appropriate recovery method
        let pg_version = self.detect_pg_version()?;
        info!("Detected PostgreSQL version: {}", pg_version);

        if pg_version >= 12 {
            // PostgreSQL 12+ uses postgresql.conf and recovery.signal
            self.configure_recovery_v12_plus()?;
        } else {
            // PostgreSQL < 12 uses recovery.conf
            self.configure_recovery_legacy()?;
        }

        Ok(())
    }

    /// Configure recovery for PostgreSQL 12+.
    fn configure_recovery_v12_plus(&self) -> Result<(), PostgresError> {
        // Create recovery.signal file
        let signal_path = self.target_dir.join("recovery.signal");
        fs::File::create(&signal_path).map_err(PostgresError::Io)?;
        info!("Created recovery.signal");

        // Update postgresql.conf with recovery settings
        let conf_path = self.target_dir.join("postgresql.conf");
        let mut conf_content = if conf_path.exists() {
            fs::read_to_string(&conf_path).map_err(PostgresError::Io)?
        } else {
            String::new()
        };

        // Add recovery settings
        conf_content.push_str("\n# PITR Recovery Settings (added by Warden)\n");
        conf_content.push_str(&format!(
            "restore_command = 'cp {}/pg_wal/%f %p'\n",
            self.target_dir.to_string_lossy()
        ));

        match &self.plan.target {
            RecoveryTarget::Time(t) => {
                let target_time_str = t.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string();
                conf_content.push_str(&format!("recovery_target_time = '{}'\n", target_time_str));
            }
            RecoveryTarget::Lsn(lsn) => {
                conf_content.push_str(&format!("recovery_target_lsn = '{}'\n", lsn));
            }
            RecoveryTarget::RestorePoint(name) => {
                conf_content.push_str(&format!("recovery_target_name = '{}'\n", name));
            }
            RecoveryTarget::Latest => {
                // No target - recover to end of WAL
            }
        }

        conf_content.push_str("recovery_target_action = 'pause'\n");
        conf_content.push_str("recovery_target_inclusive = true\n");
        conf_content.push_str("recovery_target_timeline = 'latest'\n");

        fs::write(&conf_path, conf_content).map_err(PostgresError::Io)?;
        info!("Updated postgresql.conf with recovery settings");

        Ok(())
    }

    /// Configure recovery for PostgreSQL < 12.
    fn configure_recovery_legacy(&self) -> Result<(), PostgresError> {
        let recovery_conf_path = self.target_dir.join("recovery.conf");

        let mut content = String::new();
        content.push_str("# PITR Recovery Configuration (generated by Warden)\n");
        content.push_str(&format!(
            "restore_command = 'cp {}/pg_wal/%f %p'\n",
            self.target_dir.to_string_lossy()
        ));

        match &self.plan.target {
            RecoveryTarget::Time(t) => {
                let target_time_str = t.format("%Y-%m-%d %H:%M:%S%.6f+00").to_string();
                content.push_str(&format!("recovery_target_time = '{}'\n", target_time_str));
            }
            RecoveryTarget::Lsn(lsn) => {
                content.push_str(&format!("recovery_target_lsn = '{}'\n", lsn));
            }
            RecoveryTarget::RestorePoint(name) => {
                content.push_str(&format!("recovery_target_name = '{}'\n", name));
            }
            RecoveryTarget::Latest => {
                // No target
            }
        }

        content.push_str("recovery_target_inclusive = true\n");
        content.push_str("recovery_target_timeline = 'latest'\n");
        content.push_str("pause_at_recovery_target = true\n");

        fs::write(&recovery_conf_path, content).map_err(PostgresError::Io)?;
        info!("Created recovery.conf");

        Ok(())
    }

    /// Detect PostgreSQL version from the data directory.
    fn detect_pg_version(&self) -> Result<u32, PostgresError> {
        let version_file = self.target_dir.join("PG_VERSION");
        if version_file.exists() {
            let content = fs::read_to_string(&version_file).map_err(PostgresError::Io)?;
            let version_str = content.trim();

            // Parse major version (e.g., "15" or "14.1")
            let major_version = version_str
                .split('.')
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(15); // Default to 15 if parsing fails

            return Ok(major_version);
        }

        // Try to get version from server_version in backup metadata
        let version_str = &self.plan.base_backup.server_version;
        if let Some(major) = version_str
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|v| v.parse::<u32>().ok())
        {
            return Ok(major);
        }

        // Default to PostgreSQL 15
        Ok(15)
    }

    /// Get the recovery mode description.
    fn get_recovery_mode(&self) -> String {
        match &self.plan.target {
            RecoveryTarget::Time(t) => format!("time-based ({})", t),
            RecoveryTarget::Lsn(lsn) => format!("LSN-based ({})", lsn),
            RecoveryTarget::RestorePoint(name) => format!("restore-point ({})", name),
            RecoveryTarget::Latest => "latest".to_string(),
        }
    }

    /// Start PostgreSQL in recovery mode.
    fn start_recovery(&self) -> Result<(), PostgresError> {
        info!("Starting PostgreSQL in recovery mode");

        let pg_ctl = self
            .pg_bin_dir
            .as_ref()
            .map(|d| d.join("pg_ctl"))
            .unwrap_or_else(|| PathBuf::from("pg_ctl"));

        let status = Command::new(&pg_ctl)
            .arg("start")
            .arg("-D")
            .arg(&self.target_dir)
            .arg("-w") // Wait for startup
            .arg("-l")
            .arg(self.target_dir.join("postgresql.log"))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();

        match status {
            Ok(s) if s.success() => {
                info!("PostgreSQL started successfully in recovery mode");
                Ok(())
            }
            Ok(s) => {
                error!("pg_ctl exited with status: {}", s);
                Err(PostgresError::RestoreError(format!(
                    "Failed to start PostgreSQL: exit code {}",
                    s
                )))
            }
            Err(e) => {
                error!("Failed to execute pg_ctl: {}", e);
                Err(PostgresError::RestoreError(format!(
                    "Failed to start PostgreSQL: {}",
                    e
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_plan() -> RecoveryPlan {
        use super::super::types::*;

        RecoveryPlan {
            id: Uuid::new_v4(),
            computed_at: Utc::now(),
            target: RecoveryTarget::Time(Utc::now()),
            base_backup: BaseBackupInfo {
                id: Uuid::new_v4(),
                path: "/test/backup".to_string(),
                start_time: Utc::now() - chrono::Duration::hours(1),
                end_time: Some(Utc::now()),
                wal_start: Some("0/1000000".to_string()),
                wal_end: Some("0/2000000".to_string()),
                server_version: "15.0".to_string(),
                size_bytes: 1024,
                is_remote: false,
            },
            wal_segments: Vec::new(),
            recovery_window: RecoveryWindow {
                earliest: Utc::now() - chrono::Duration::hours(1),
                latest: Some(Utc::now()),
                target_in_window: true,
            },
            validation: PlanValidation::valid(),
            estimated_download_bytes: 0,
        }
    }

    #[test]
    fn test_executor_creation() {
        let temp_dir = TempDir::new().unwrap();
        let plan = create_test_plan();
        let executor = PitrExecutor::new(plan, temp_dir.path().to_path_buf());
        assert!(!executor.auto_start);
    }

    #[test]
    fn test_prepare_target_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("recovery");
        let plan = create_test_plan();
        let executor = PitrExecutor::new(plan, target.clone());

        executor.prepare_target_directory().unwrap();
        assert!(target.exists());
    }

    #[test]
    fn test_detect_pg_version_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().to_path_buf();

        // Create PG_VERSION file
        fs::write(target.join("PG_VERSION"), "15\n").unwrap();

        let plan = create_test_plan();
        let executor = PitrExecutor::new(plan, target);

        let version = executor.detect_pg_version().unwrap();
        assert_eq!(version, 15);
    }

    #[test]
    fn test_recovery_mode_string() {
        let temp_dir = TempDir::new().unwrap();
        let mut plan = create_test_plan();
        plan.target = RecoveryTarget::Latest;

        let executor = PitrExecutor::new(plan, temp_dir.path().to_path_buf());
        assert_eq!(executor.get_recovery_mode(), "latest");
    }
}
