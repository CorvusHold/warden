use anyhow::{Context, Result};
use log::{debug, info};
use std::process::{Command, Stdio};

/// Format options for pg_dump
pub enum PgDumpFormat {
    Plain,
    Custom,
    Directory,
    Tar,
}

impl PgDumpFormat {
    fn as_str(&self) -> &'static str {
        match self {
            PgDumpFormat::Plain => "p",
            PgDumpFormat::Custom => "c",
            PgDumpFormat::Directory => "d",
            PgDumpFormat::Tar => "t",
        }
    }
}

/// Options for pg_dump command
pub struct PgDumpOptions {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
    pub file: String,
    pub format: PgDumpFormat,
    pub compress: Option<i32>,
    pub schema_only: bool,
    pub data_only: bool,
    pub clean: bool,
    pub if_exists: bool,
    pub verbose: bool,
    pub schemas: Vec<String>,
    pub tables: Vec<String>,
    pub exclude_tables: Vec<String>,
}

impl Default for PgDumpOptions {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            username: "postgres".to_string(),
            password: "".to_string(),
            database: "postgres".to_string(),
            file: "dump.dump".to_string(),
            format: PgDumpFormat::Custom,
            compress: Some(9),
            schema_only: false,
            data_only: false,
            clean: true,
            if_exists: true,
            verbose: false,
            schemas: Vec::new(),
            tables: Vec::new(),
            exclude_tables: Vec::new(),
        }
    }
}

/// Wrapper for pg_dump command
pub struct PgDump;

impl PgDump {
    /// Run pg_dump with the given options
    pub fn run(options: &PgDumpOptions) -> Result<()> {
        let mut cmd = Command::new("pg_dump");

        // Set PGPASSWORD environment variable
        cmd.env("PGPASSWORD", &options.password);

        cmd.arg("--host")
            .arg(&options.host)
            .arg("--port")
            .arg(options.port.to_string())
            .arg("--username")
            .arg(&options.username)
            .arg("--dbname")
            .arg(&options.database)
            .arg("--file")
            .arg(&options.file)
            .arg("--format")
            .arg(options.format.as_str());

        if let Some(compress) = options.compress {
            cmd.arg("--compress").arg(compress.to_string());
        }

        if options.schema_only {
            cmd.arg("--schema-only");
        }

        if options.data_only {
            cmd.arg("--data-only");
        }

        if options.clean {
            cmd.arg("--clean");
        }

        if options.if_exists {
            cmd.arg("--if-exists");
        }

        if options.verbose {
            cmd.arg("--verbose");
        }

        for schema in &options.schemas {
            cmd.arg("--schema").arg(schema);
        }

        for table in &options.tables {
            cmd.arg("--table").arg(table);
        }

        for table in &options.exclude_tables {
            cmd.arg("--exclude-table").arg(table);
        }

        debug!("Running pg_dump command: {cmd:?}");

        let output = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .output()
            .context("Failed to execute pg_dump")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("pg_dump failed: {}", stderr);
        }

        info!("pg_dump completed successfully");
        Ok(())
    }

    /// Check if pg_dump is available in the system
    pub fn check_availability() -> Result<()> {
        let output = Command::new("pg_dump")
            .arg("--version")
            .output()
            .context("Failed to execute pg_dump")?;

        if !output.status.success() {
            anyhow::bail!("pg_dump is not available");
        }

        let version = String::from_utf8_lossy(&output.stdout);
        debug!("pg_dump version: {version}");

        Ok(())
    }
}
