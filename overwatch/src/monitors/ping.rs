use chrono::Utc;
use reqwest::Url;
use std::process::Command;
use std::time::Instant;

use crate::error::Error;
use crate::models::service::{MonitorResult, MonitorType, Service};

pub async fn exec(service: &Service) -> Result<MonitorResult, Error> {
    if service.monitor_type != MonitorType::PING {
        return Err(Error::InvalidServiceConfig(
            "Service is not a PING monitor".to_string(),
        ));
    }

    let beginning = Instant::now();
    let result = check_ping(service).await;
    let response_time = beginning.elapsed().as_millis();

    match result {
        Ok(details) => Ok(MonitorResult {
            service_id: service.id.clone(),
            timestamp: Utc::now(),
            success: true,
            response_time,
            error: None,
            details: Some(details),
        }),
        Err(err) => Ok(MonitorResult {
            service_id: service.id.clone(),
            timestamp: Utc::now(),
            success: false,
            response_time,
            error: Some(err.to_string()),
            details: None,
        }),
    }
}

async fn check_ping(service: &Service) -> Result<String, Error> {
    // Parse URL to extract hostname
    let url = Url::parse(&service.url)
        .map_err(|e| Error::InvalidServiceConfig(format!("Invalid URL: {}", e)))?;

    let hostname = url
        .host_str()
        .ok_or_else(|| Error::InvalidServiceConfig("URL does not contain a hostname".to_string()))?
        .to_string();

    // Determine ping count (default to 4)
    let count = service.ping_count.unwrap_or(4);

    // Use the ping command with the specified count
    let output = if cfg!(target_os = "windows") {
        Command::new("ping")
            .args(["-n", &count.to_string(), &hostname])
            .output()
    } else {
        Command::new("ping")
            .args(["-c", &count.to_string(), &hostname])
            .output()
    }
    .map_err(|e| Error::Ping(format!("Failed to execute ping command: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    // Check if ping was successful
    if !output.status.success() {
        return Err(Error::Ping(format!(
            "Ping failed with exit code {}: {}",
            output.status.code().unwrap_or(-1),
            stderr
        )));
    }

    // Parse ping statistics
    let stats = parse_ping_stats(&stdout);

    Ok(format!("Ping to {} successful. {}", hostname, stats))
}

fn parse_ping_stats(output: &str) -> String {
    // This is a simple parser that extracts common ping statistics
    // It tries to be cross-platform but might need adjustments

    let mut stats = Vec::new();

    // Look for packet loss
    if let Some(loss_line) = output.lines().find(|line| line.contains("packet loss")) {
        if let Some(loss) = loss_line.split_whitespace().find(|word| word.contains('%')) {
            stats.push(format!("Packet loss: {}", loss));
        }
    }

    // Look for round-trip times
    if let Some(rtt_line) = output.lines().find(|line| {
        line.contains("min/avg/max") || line.contains("round-trip") || line.contains("rtt")
    }) {
        stats.push(format!("RTT: {}", rtt_line.trim()));
    }

    if stats.is_empty() {
        "No detailed statistics available".to_string()
    } else {
        stats.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_valid_ping() {
        let service = Service {
            id: "test-ping-1".to_string(),
            name: "Test Ping Service".to_string(),
            monitor_type: MonitorType::PING,
            url: "https://corvushold.com".to_string(),
            http_method: None,
            payload: None,
            headers: None,
            verify_ssl: None,
            expected_status_code: None,
            expected_body: None,
            dns_record_type: None,
            expected_ip: None,
            ping_count: Some(2),
            interval: 60,
            timeout: 10,
            retry: 3,
        };

        let result = exec(&service).await;
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn test_invalid_host_ping() {
        let service = Service {
            id: "test-ping-2".to_string(),
            name: "Test Invalid Ping Service".to_string(),
            monitor_type: MonitorType::PING,
            url: "https://this-domain-does-not-exist-12345.com".to_string(),
            http_method: None,
            payload: None,
            headers: None,
            verify_ssl: None,
            expected_status_code: None,
            expected_body: None,
            dns_record_type: None,
            expected_ip: None,
            ping_count: Some(1),
            interval: 60,
            timeout: 5,
            retry: 1,
        };

        let result = exec(&service).await;
        assert!(result.is_ok());
        assert!(!result.unwrap().success);
    }
}
