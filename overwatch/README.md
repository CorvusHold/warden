# Overwatch

A Rust library for monitoring HTTP, DNS, and PING services.

## Features

- **HTTP Monitoring**: Monitor HTTP endpoints with customizable methods, headers, and payload
  - SSL certificate verification
  - Status code validation
  - Response body validation
  
- **DNS Monitoring**: Monitor DNS records
  - Support for various record types (A, AAAA, CNAME, MX, TXT, NS, SOA, SRV)
  - Expected IP validation
  
- **PING Monitoring**: Monitor host availability using PING
  - Customizable ping count
  - Response time and packet loss statistics

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
overwatch = { path = "../path/to/overwatch" }
```

### Example

```rust
use overwatch::{Service, MonitorType, HttpRequestMethod};

#[tokio::main]
async fn main() {
    // Create an HTTP monitoring service
    let http_service = Service {
        id: "http-service".to_string(),
        name: "Google HTTP Service".to_string(),
        monitor_type: MonitorType::HTTP,
        url: "https://corvushold.com".to_string(),
        http_method: Some(HttpRequestMethod::GET),
        payload: None,
        headers: None,
        verify_ssl: Some(true),
        expected_status_code: Some(200),
        expected_body: None,
        dns_record_type: None,
        expected_ip: None,
        ping_count: None,
        interval: 60,
        timeout: 10,
        retry: 3,
    };
    
    // Execute the monitoring check
    match http_service.exec().await {
        Ok(result) => {
            info!("Monitoring successful: {}", result.success);
            info!("Response time: {}ms", result.response_time);
            if let Some(details) = result.details {
                info!("Details: {}", details);
            }
            if let Some(error) = result.error {
                info!("Error: {}", error);
            }
        },
        Err(e) => {
            info!("Failed to execute monitoring: {}", e);
        }
    }
}
```

## Testing

Run the tests with:

```bash
cargo test
```

## License

MIT
