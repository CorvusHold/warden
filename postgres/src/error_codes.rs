//! Exit codes and error handling utilities for Warden CLI commands.
//!
//! These exit codes are documented in docs/API.md and should be used consistently
//! across all CLI commands.

use std::fmt;

/// CLI exit codes as documented in API.md Section 6.1
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// Success - operation completed successfully
    Success = 0,
    /// Usage error - bad flags, missing arguments, invalid syntax
    UsageError = 1,
    /// Configuration error - invalid config file, missing required config
    ConfigError = 2,
    /// Environment error - network issues, missing tools, disk full
    EnvironmentError = 3,
    /// Remote service error - S3 failures, C2 connection issues
    RemoteServiceError = 4,
    /// Internal error - unexpected bug, assertion failure
    InternalError = 5,
}

impl ExitCode {
    /// Convert to i32 for use with std::process::exit
    pub fn code(self) -> i32 {
        self as i32
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExitCode::Success => write!(f, "Success"),
            ExitCode::UsageError => write!(f, "Usage error"),
            ExitCode::ConfigError => write!(f, "Configuration error"),
            ExitCode::EnvironmentError => write!(f, "Environment error"),
            ExitCode::RemoteServiceError => write!(f, "Remote service error"),
            ExitCode::InternalError => write!(f, "Internal error"),
        }
    }
}

/// Categorize an error and return the appropriate exit code
pub fn categorize_error(error: &anyhow::Error) -> ExitCode {
    let error_str = error.to_string().to_lowercase();

    // Check for specific error patterns
    if error_str.contains("usage")
        || error_str.contains("invalid argument")
        || error_str.contains("missing required argument")
        || error_str.contains("unrecognized option")
        || error_str.contains("unrecognized argument")
    {
        return ExitCode::UsageError;
    }

    if error_str.contains("config")
        || error_str.contains("configuration")
        || error_str.contains("policy file")
        || error_str.contains("invalid format")
    {
        return ExitCode::ConfigError;
    }

    if error_str.contains("network")
        || error_str.contains("connection refused")
        || error_str.contains("timeout")
        || error_str.contains("disk full")
        || error_str.contains("no space")
        || error_str.contains("permission denied")
        || error_str.contains("pg_dump")
        || error_str.contains("pg_restore")
        || error_str.contains("pg_basebackup")
        || error_str.contains("ssh")
        || error_str.contains("tunnel")
    {
        return ExitCode::EnvironmentError;
    }

    if error_str.contains("s3")
        || error_str.contains("minio")
        || error_str.contains("bucket")
        || error_str.contains("access denied")
        || error_str.contains("not found")
        || error_str.contains("upload failed")
        || error_str.contains("download failed")
        || error_str.contains("storage")
    {
        return ExitCode::RemoteServiceError;
    }

    // Default to internal error for unexpected issues
    ExitCode::InternalError
}

/// Format an error message for CLI output
pub fn format_error_message(error: &anyhow::Error, exit_code: ExitCode) -> String {
    format!(
        "Error: {} (exit code: {})\n\nDetails: {}",
        exit_code,
        exit_code.code(),
        error
    )
}

/// Exit with the appropriate code after logging the error
pub fn exit_with_error(error: anyhow::Error) -> ! {
    let exit_code = categorize_error(&error);
    let message = format_error_message(&error, exit_code);
    eprintln!("{}", message);
    log::error!("{}", error);
    std::process::exit(exit_code.code())
}

/// Result type that can be converted to an exit code
pub type CliResult<T> = Result<T, CliError>;

/// CLI error with associated exit code
#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub exit_code: ExitCode,
    pub source: Option<anyhow::Error>,
}

impl CliError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: ExitCode::UsageError,
            source: None,
        }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: ExitCode::ConfigError,
            source: None,
        }
    }

    pub fn environment(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: ExitCode::EnvironmentError,
            source: None,
        }
    }

    pub fn remote_service(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: ExitCode::RemoteServiceError,
            source: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: ExitCode::InternalError,
            source: None,
        }
    }

    pub fn with_source(mut self, source: anyhow::Error) -> Self {
        self.source = Some(source);
        self
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref source) = self.source {
            write!(f, ": {}", source)?;
        }
        Ok(())
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl From<anyhow::Error> for CliError {
    fn from(error: anyhow::Error) -> Self {
        let exit_code = categorize_error(&error);
        Self {
            message: error.to_string(),
            exit_code,
            source: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_exit_codes_are_unique() {
        let codes = [
            ExitCode::Success,
            ExitCode::UsageError,
            ExitCode::ConfigError,
            ExitCode::EnvironmentError,
            ExitCode::RemoteServiceError,
            ExitCode::InternalError,
        ];

        for (i, code1) in codes.iter().enumerate() {
            for (j, code2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(code1.code(), code2.code());
                }
            }
        }
    }

    #[test]
    fn test_categorize_usage_error() {
        let error = anyhow!("Invalid argument: --foo is not recognized");
        assert_eq!(categorize_error(&error), ExitCode::UsageError);
    }

    #[test]
    fn test_categorize_config_error() {
        let error = anyhow!("Configuration file not found");
        assert_eq!(categorize_error(&error), ExitCode::ConfigError);
    }

    #[test]
    fn test_categorize_environment_error() {
        let error = anyhow!("SSH tunnel connection refused");
        assert_eq!(categorize_error(&error), ExitCode::EnvironmentError);
    }

    #[test]
    fn test_categorize_remote_service_error() {
        let error = anyhow!("S3 bucket not found");
        assert_eq!(categorize_error(&error), ExitCode::RemoteServiceError);
    }

    #[test]
    fn test_categorize_internal_error() {
        let error = anyhow!("Unexpected state in backup manager");
        assert_eq!(categorize_error(&error), ExitCode::InternalError);
    }

    #[test]
    fn test_cli_error_constructors() {
        let usage = CliError::usage("Bad argument");
        assert_eq!(usage.exit_code, ExitCode::UsageError);

        let config = CliError::config("Invalid config");
        assert_eq!(config.exit_code, ExitCode::ConfigError);

        let env = CliError::environment("Network error");
        assert_eq!(env.exit_code, ExitCode::EnvironmentError);

        let remote = CliError::remote_service("S3 error");
        assert_eq!(remote.exit_code, ExitCode::RemoteServiceError);

        let internal = CliError::internal("Bug");
        assert_eq!(internal.exit_code, ExitCode::InternalError);
    }
}
