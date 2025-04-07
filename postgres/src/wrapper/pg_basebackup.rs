use anyhow::{Context, Result};
use log::{debug, info};
use std::process::{Command, Stdio};

/// Options for pg_basebackup command
pub struct PgBaseBackupOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub pgdata: String,
    pub format: String,
    pub checkpoint: String,
    pub wal_method: String,
    pub compress: Option<String>,
    pub label: Option<String>,
    pub progress: bool,
    pub verbose: bool,
}

impl Default for PgBaseBackupOptions {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            pgdata: ".".to_string(),
            format: "plain".to_string(),
            checkpoint: "fast".to_string(),
            wal_method: "stream".to_string(),
            compress: None,
            label: None,
            progress: true,
            verbose: false,
        }
    }
}

/// Wrapper for pg_basebackup command
pub struct PgBaseBackup;

impl PgBaseBackup {
    /// Run pg_basebackup with the given options
    pub fn run(options: &PgBaseBackupOptions) -> Result<()> {
        let mut cmd = Command::new("pg_basebackup");

        cmd.arg("--host")
            .arg(&options.host)
            .arg("--port")
            .arg(options.port.to_string())
            .arg("--username")
            .arg(&options.username)
            .arg("--pgdata")
            .arg(&options.pgdata)
            .arg("--format")
            .arg(&options.format)
            .arg("--checkpoint")
            .arg(&options.checkpoint)
            .arg("--wal-method")
            .arg(&options.wal_method);

        if let Some(compress) = &options.compress {
            cmd.arg("--compress").arg(compress);
        }

        if let Some(label) = &options.label {
            cmd.arg("--label").arg(label);
        }

        if options.progress {
            cmd.arg("--progress");
        }

        if options.verbose {
            cmd.arg("--verbose");
        }

        debug!("Running pg_basebackup command: {:?}", cmd);

        let output = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .context("Failed to execute pg_basebackup")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("pg_basebackup failed: {}", stderr);
        }

        info!("pg_basebackup completed successfully");
        Ok(())
    }

    /// Check if pg_basebackup is available in the system
    pub fn check_availability() -> Result<()> {
        let output = Command::new("pg_basebackup")
            .arg("--version")
            .output()
            .context("Failed to execute pg_basebackup")?;

        if !output.status.success() {
            anyhow::bail!("pg_basebackup is not available");
        }

        let version = String::from_utf8_lossy(&output.stdout);
        debug!("pg_basebackup version: {}", version);

        Ok(())
    }
}
