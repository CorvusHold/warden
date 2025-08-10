use crate::common::PostgresConfig;
use crate::PostgresError;
use log::{debug, error, info};
use std::path::Path;
use std::process::{Command, Stdio};

/// Wrapper for pg_restore utility
pub struct PgRestore {
    config: PostgresConfig,
}

impl PgRestore {
    /// Create a new PgRestore instance
    pub fn new(config: PostgresConfig) -> Self {
        Self { config }
    }

    /// Restore a database from a dump file
    pub async fn restore<P: AsRef<Path>>(
        &self,
        dump_file: P,
        target_db: Option<&str>,
    ) -> Result<(), PostgresError> {
        let dump_file = dump_file.as_ref();
        info!("Restoring database from dump file: {dump_file:?}");

        if !dump_file.exists() {
            return Err(PostgresError::RestoreError(format!(
                "Dump file does not exist: {dump_file:?}"
            )));
        }

        // Build pg_restore command
        let mut cmd = Command::new("pg_restore");

        // Add connection options
        cmd.arg("--host")
            .arg(&self.config.host)
            .arg("--port")
            .arg(self.config.port.to_string())
            .arg("--username")
            .arg(&self.config.user);

        // Add target database if specified
        if let Some(db) = target_db {
            cmd.arg("--dbname").arg(db);
        } else {
            cmd.arg("--dbname").arg(&self.config.database);
        }

        // Add dump file
        cmd.arg(dump_file);

        // Add options
        cmd.arg("--verbose")
            .arg("--no-owner")
            .arg("--no-privileges");

        // Set password environment variable if provided
        if let Some(password) = &self.config.password {
            cmd.env("PGPASSWORD", password);
        }

        debug!("Running pg_restore command: {cmd:?}");

        // Execute command
        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(PostgresError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("pg_restore failed: {stderr}");
            return Err(PostgresError::RestoreError(format!(
                "pg_restore failed: {stderr}"
            )));
        }

        info!("Database restored successfully from dump file: {dump_file:?}");
        Ok(())
    }

    /// Restore a database from a dump file with custom options
    pub async fn restore_with_options<P: AsRef<Path>>(
        &self,
        dump_file: P,
        target_db: Option<&str>,
        options: &[&str],
    ) -> Result<(), PostgresError> {
        let dump_file = dump_file.as_ref();
        info!("Restoring database from dump file with custom options: {dump_file:?}");

        if !dump_file.exists() {
            return Err(PostgresError::RestoreError(format!(
                "Dump file does not exist: {dump_file:?}"
            )));
        }

        // Build pg_restore command
        let mut cmd = Command::new("pg_restore");

        // Add connection options
        cmd.arg("--host")
            .arg(&self.config.host)
            .arg("--port")
            .arg(self.config.port.to_string())
            .arg("--username")
            .arg(&self.config.user);

        // Add target database if specified
        if let Some(db) = target_db {
            cmd.arg("--dbname").arg(db);
        } else {
            cmd.arg("--dbname").arg(&self.config.database);
        }

        // Add dump file
        cmd.arg(dump_file);

        // Add custom options
        for option in options {
            cmd.arg(option);
        }

        // Set password environment variable if provided
        if let Some(password) = &self.config.password {
            cmd.env("PGPASSWORD", password);
        }

        debug!("Running pg_restore command: {cmd:?}");

        // Execute command
        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(PostgresError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("pg_restore failed: {stderr}");
            return Err(PostgresError::RestoreError(format!(
                "pg_restore failed: {stderr}"
            )));
        }

        info!("Database restored successfully from dump file: {dump_file:?}");
        Ok(())
    }

    /// List the contents of a dump file
    pub async fn list_contents<P: AsRef<Path>>(
        &self,
        dump_file: P,
    ) -> Result<String, PostgresError> {
        let dump_file = dump_file.as_ref();
        info!("Listing contents of dump file: {dump_file:?}");

        if !dump_file.exists() {
            return Err(PostgresError::RestoreError(format!(
                "Dump file does not exist: {dump_file:?}"
            )));
        }

        // Build pg_restore command
        let mut cmd = Command::new("pg_restore");

        // Add options
        cmd.arg("--list").arg(dump_file);

        debug!("Running pg_restore --list command: {cmd:?}");

        // Execute command
        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(PostgresError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!("pg_restore --list failed: {stderr}");
            return Err(PostgresError::RestoreError(format!(
                "pg_restore --list failed: {stderr}"
            )));
        }

        let contents = String::from_utf8_lossy(&output.stdout).to_string();
        info!("Successfully listed contents of dump file: {dump_file:?}");

        Ok(contents)
    }
}
