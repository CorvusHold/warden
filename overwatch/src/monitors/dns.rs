use chrono::Utc;
use reqwest::Url;
use std::time::{Duration, Instant};
use trust_dns_resolver::config::ResolverOpts;
use trust_dns_resolver::proto::rr::RecordType;
use trust_dns_resolver::TokioAsyncResolver;

use crate::error::Error;
use crate::models::service::{MonitorResult, MonitorType, Service};

pub async fn exec(service: &Service) -> Result<MonitorResult, Error> {
    if service.monitor_type != MonitorType::DNS {
        return Err(Error::InvalidServiceConfig(
            "Service is not a DNS monitor".to_string(),
        ));
    }

    let beginning = Instant::now();
    let result = check_dns(service).await;
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

async fn check_dns(service: &Service) -> Result<String, Error> {
    // Parse URL to extract hostname
    let url = Url::parse(&service.url)
        .map_err(|e| Error::InvalidServiceConfig(format!("Invalid URL: {e}")))?;

    let hostname = url
        .host_str()
        .ok_or_else(|| Error::InvalidServiceConfig("URL does not contain a hostname".to_string()))?
        .to_string();

    // Determine record type (default to A)
    let record_type = match service.dns_record_type.as_deref() {
        Some("A") | None => RecordType::A,
        Some("AAAA") => RecordType::AAAA,
        Some("CNAME") => RecordType::CNAME,
        Some("MX") => RecordType::MX,
        Some("TXT") => RecordType::TXT,
        Some("NS") => RecordType::NS,
        Some("SOA") => RecordType::SOA,
        Some("SRV") => RecordType::SRV,
        Some(rt) => {
            return Err(Error::DnsRecordType(format!(
                "Unsupported DNS record type: {rt}"
            )))
        }
    };

    // Create resolver with timeout
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(service.timeout as u64);

    // Create the resolver
    let resolver = TokioAsyncResolver::tokio_from_system_conf()
        .map_err(|e| Error::DnsResolution(format!("Failed to create resolver: {e}")))?;

    // Perform lookup
    let response = resolver
        .lookup(hostname.clone(), record_type)
        .await
        .map_err(|e| Error::DnsResolution(format!("DNS lookup failed: {e}")))?;

    // Get all records as strings
    let records: Vec<String> = response.iter().map(|r| r.to_string()).collect();

    // Check expected IP if specified
    if let Some(expected_ip) = &service.expected_ip {
        if !records.iter().any(|r| r.contains(expected_ip)) {
            return Err(Error::DnsExpectedIp(format!(
                "Expected IP {expected_ip} not found in DNS records: {records:?}"
            )));
        }
    }

    Ok(format!(
        "DNS lookup for {hostname} ({record_type:?}) successful. Records: {records:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::service::MonitorType;

    #[tokio::test]
    async fn test_valid_dns_lookup() {
        let service = Service {
            id: "test-dns-1".to_string(),
            name: "Test DNS Service".to_string(),
            monitor_type: MonitorType::DNS,
            url: "https://corvushold.com".to_string(),
            http_method: None,
            payload: None,
            headers: None,
            verify_ssl: None,
            expected_status_code: None,
            expected_body: None,
            dns_record_type: Some("A".to_string()),
            expected_ip: None,
            ping_count: None,
            interval: 60,
            timeout: 10,
            retry: 3,
        };

        let result = exec(&service).await;
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    #[tokio::test]
    async fn test_invalid_hostname() {
        let service = Service {
            id: "test-dns-2".to_string(),
            name: "Test Invalid DNS Service".to_string(),
            monitor_type: MonitorType::DNS,
            url: "https://this-domain-does-not-exist-12345.com".to_string(),
            http_method: None,
            payload: None,
            headers: None,
            verify_ssl: None,
            expected_status_code: None,
            expected_body: None,
            dns_record_type: Some("A".to_string()),
            expected_ip: None,
            ping_count: None,
            interval: 60,
            timeout: 10,
            retry: 3,
        };

        let result = exec(&service).await;
        assert!(result.is_ok());
        assert!(!result.unwrap().success);
    }
}
