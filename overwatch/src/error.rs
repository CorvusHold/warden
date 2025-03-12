use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("DNS resolution error: {0}")]
    DnsResolution(String),

    #[error("DNS record type error: {0}")]
    DnsRecordType(String),

    #[error("DNS expected IP error: {0}")]
    DnsExpectedIp(String),

    #[error("Ping error: {0}")]
    Ping(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SSL verification error: {0}")]
    SslVerification(String),

    #[error("Timeout error")]
    Timeout,

    #[error("Invalid service configuration: {0}")]
    InvalidServiceConfig(String),

    #[error("Unexpected status code: expected {expected}, got {actual}")]
    UnexpectedStatusCode { expected: u16, actual: u16 },

    #[error("Expected body not found")]
    ExpectedBodyNotFound,

    #[error("Other error: {0}")]
    Other(String),
}
