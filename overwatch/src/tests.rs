use crate::{HttpRequestMethod, MonitorType, Service};

mod http_tests {
    use super::*;

    #[tokio::test]
    async fn test_http_get_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("Hello world!")
            .create_async()
            .await;

        let service = Service {
            id: "test-http-1".to_string(),
            name: "Test HTTP Service".to_string(),
            monitor_type: MonitorType::HTTP,
            url: format!("{}/test", server.url()),
            http_method: Some(HttpRequestMethod::GET),
            payload: None,
            headers: None,
            verify_ssl: Some(false),
            expected_status_code: Some(200),
            expected_body: None,
            dns_record_type: None,
            expected_ip: None,
            ping_count: None,
            interval: 60,
            timeout: 10,
            retry: 3,
        };

        let result = service.exec().await;
        assert!(result.is_ok());
        let monitor_result = result.unwrap();
        assert!(monitor_result.success);
    }

    #[tokio::test]
    async fn test_http_post_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/test")
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"created"}"#)
            .create_async()
            .await;

        let service = Service {
            id: "test-http-2".to_string(),
            name: "Test HTTP POST Service".to_string(),
            monitor_type: MonitorType::HTTP,
            url: format!("{}/test", server.url()),
            http_method: Some(HttpRequestMethod::POST),
            payload: Some(r#"{"test":"data"}"#.to_string()),
            headers: Some(vec![(
                "Content-Type".to_string(),
                "application/json".to_string(),
            )]),
            verify_ssl: Some(false),
            expected_status_code: Some(201),
            expected_body: Some("created".to_string()),
            dns_record_type: None,
            expected_ip: None,
            ping_count: None,
            interval: 60,
            timeout: 10,
            retry: 3,
        };

        let result = service.exec().await;
        assert!(result.is_ok());
        let monitor_result = result.unwrap();
        assert!(monitor_result.success);
    }

    #[tokio::test]
    async fn test_http_status_code_failure() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/error")
            .with_status(500)
            .with_header("content-type", "text/plain")
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let service = Service {
            id: "test-http-3".to_string(),
            name: "Test HTTP Error Service".to_string(),
            monitor_type: MonitorType::HTTP,
            url: format!("{}/error", server.url()),
            http_method: Some(HttpRequestMethod::GET),
            payload: None,
            headers: None,
            verify_ssl: Some(false),
            expected_status_code: Some(200),
            expected_body: None,
            dns_record_type: None,
            expected_ip: None,
            ping_count: None,
            interval: 60,
            timeout: 10,
            retry: 3,
        };

        let result = service.exec().await;
        assert!(result.is_ok());
        let monitor_result = result.unwrap();
        assert!(!monitor_result.success);
    }
}

mod dns_tests {
    use super::*;

    /// Tests DNS monitoring by performing an "A" record lookup on a valid domain and asserting the result is successful.
    async fn test_dns_lookup() {
        let service = Service {
            id: "test-dns-1".to_string(),
            name: "Test DNS Service".to_string(),
            monitor_type: MonitorType::DNS,
            url: "https://corvushold.com/".to_string(),
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

        let result = service.exec().await;
        assert!(result.is_ok());
        let monitor_result = result.unwrap();
        assert!(monitor_result.success);
    }

    #[tokio::test]
    async fn test_dns_nonexistent_domain() {
        let service = Service {
            id: "test-dns-2".to_string(),
            name: "Test DNS Nonexistent Service".to_string(),
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

        let result = service.exec().await;
        assert!(result.is_ok());
        let monitor_result = result.unwrap();
        assert!(!monitor_result.success);
    }
}

mod ping_tests {
    use super::*;

    /// Tests that the Ping monitor successfully pings a valid domain.
    ///
    /// This test creates a `Service` configured for Ping monitoring with two ping attempts to "https://corvushold.com". It executes the service and asserts that the result indicates a successful ping.
    ///
    /// # Examples
    ///
    /// ```
    /// // Runs as part of the test suite; not intended for direct invocation.
    /// ```
    async fn test_ping() {
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

        let result = service.exec().await;
        assert!(result.is_ok());
        let monitor_result = result.unwrap();
        assert!(monitor_result.success);
    }

    #[tokio::test]
    async fn test_ping_nonexistent_domain() {
        let service = Service {
            id: "test-ping-2".to_string(),
            name: "Test Ping Nonexistent Service".to_string(),
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

        let result = service.exec().await;
        assert!(result.is_ok());
        let monitor_result = result.unwrap();
        assert!(!monitor_result.success);
    }
}

// Integration tests that exercise multiple monitor types
mod integration_tests {
    use super::*;

    /// Tests the execution of multiple monitoring services (HTTP, DNS, and Ping) and verifies that each completes successfully.
    ///
    /// This integration test creates three `Service` instances, each configured for a different monitor type targeting the same domain. It executes each service asynchronously, asserts that all executions succeed, and checks that all results indicate success.
    ///
    /// # Examples
    ///
    /// ```
    /// // This test runs automatically with `cargo test` and does not require manual invocation.
    /// ```
    async fn test_multiple_services() {
        let services = vec![
            Service {
                id: "http-service".to_string(),
                name: "HTTP Service".to_string(),
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
            },
            Service {
                id: "dns-service".to_string(),
                name: "DNS Service".to_string(),
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
            },
            Service {
                id: "ping-service".to_string(),
                name: "Ping Service".to_string(),
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
            },
        ];

        // Execute all services and collect results
        let mut results = Vec::new();
        for service in services {
            let result = service.exec().await;
            assert!(result.is_ok());
            results.push(result.unwrap());
        }

        // Verify all services executed successfully
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.success));
    }
}
