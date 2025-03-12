use crate::error::Error;
use reqwest::{Client, Url};
use std::time::Duration;

pub async fn verify_ssl(url_str: &str) -> Result<(), Error> {
    let url = Url::parse(url_str).map_err(|e| Error::Other(format!("Invalid URL: {}", e)))?;

    // Only verify HTTPS URLs
    if url.scheme() != "https" {
        return Ok(());
    }

    // Create a client with SSL verification enabled
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Error::SslVerification(format!("Failed to create client: {}", e)))?;

    // Try to connect to the URL
    let response = client
        .head(url_str)
        .send()
        .await
        .map_err(|e| Error::SslVerification(format!("SSL verification failed: {}", e)))?;

    // If we got a response, the SSL certificate is valid
    if response.status().is_success()
        || response.status().is_redirection()
        || response.status().is_client_error()
    {
        Ok(())
    } else {
        Err(Error::SslVerification(format!(
            "Unexpected status code: {}",
            response.status()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_valid_ssl() {
        let result = verify_ssl("https://www.google.com").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_ssl() {
        // This is a test URL that has an expired or invalid certificate
        let result = verify_ssl("https://expired.badssl.com").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_non_https_url() {
        let result = verify_ssl("http://example.com").await;
        assert!(result.is_ok());
    }
}
