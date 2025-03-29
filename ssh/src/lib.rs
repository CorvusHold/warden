use thiserror::Error;

mod ssh;
pub mod cli;

pub use ssh::SSHTunnel;

#[derive(Error, Debug)]
pub enum SshError {
    #[error("SSH configuration error: {0}")]
    ConfigurationError(String),
    #[error("SSH connection error: {0}")]
    ConnectionError(String),
    #[error("SSH authentication error: {0}")]
    AuthenticationError(String),
    #[error("SSH tunnel error: {0}")]
    TunnelError(String),
}

impl From<std::io::Error> for SshError {
    fn from(err: std::io::Error) -> Self {
        SshError::ConnectionError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::ssh::SSHTunnel;

    #[test]
    fn test_new() {
        let ssh = SSHTunnel::new("localhost".to_string(), "user".to_string(), None);
        assert_eq!(ssh.host, "localhost");
        assert_eq!(ssh.user, "user");
    }
    // TO DO: Add tests
}